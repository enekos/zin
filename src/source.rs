//! Resolve component files and map model diagnostics back to their original text.
use crate::parser::{fingerprint, parse_flat, strip_comment};
use crate::{Error, Json, Result, Spec};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub(crate) const MAX_SPEC_BYTES: usize = 4 * 1024 * 1024;
const MAX_INCLUDE_DEPTH: usize = 64;
const MAX_FILES: usize = 1024;

/// A single-line source range. Columns count Unicode scalar values from one,
/// with the end column exclusive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SourceMap(Vec<Document>);

#[derive(Debug, Clone, PartialEq, Eq)]
struct Document {
    first: usize,
    path: String,
    source: String,
    starts: Vec<usize>,
}

impl Document {
    fn new(path: String, source: String, first: usize) -> Self {
        let mut starts = vec![0];
        starts.extend(source.match_indices('\n').map(|(i, _)| i + 1));
        Self {
            first,
            path,
            source,
            starts,
        }
    }

    fn span(&self, line: usize, token: Option<&str>) -> Option<SourceSpan> {
        let offset = *self.starts.get(line.checked_sub(1)?)?;
        let text = self.source[offset..].lines().next().unwrap_or("");
        let statement = strip_comment(text);
        let (start, end) = token
            .and_then(|token| {
                statement.match_indices(token).find(|(i, _)| {
                    let word = |c: char| c.is_alphanumeric() || c == '_';
                    !statement[..*i].chars().next_back().is_some_and(word)
                        && !statement[*i + token.len()..]
                            .chars()
                            .next()
                            .is_some_and(word)
                })
            })
            .map(|(i, token)| (i, i + token.len()))
            .unwrap_or_else(|| {
                (
                    statement.len() - statement.trim_start().len(),
                    statement.trim_end().len(),
                )
            });
        let column = 1 + text[..start].chars().count();
        Some(SourceSpan {
            file: self.path.clone(),
            line,
            column,
            end_column: column + text[start..end.max(start)].chars().count(),
        })
    }
}

impl SourceMap {
    pub(crate) fn inline(source: &str) -> Arc<Self> {
        Arc::new(Self(vec![Document::new(
            String::new(),
            source.to_owned(),
            1,
        )]))
    }

    pub(crate) fn span(&self, line: usize, token: Option<&str>) -> Option<SourceSpan> {
        let index = self
            .0
            .partition_point(|doc| doc.first <= line)
            .checked_sub(1)?;
        let doc = &self.0[index];
        doc.span(line - doc.first + 1, token)
    }

    pub(crate) fn locate(&self, mut error: Error) -> Error {
        if let Some(line) = error.line {
            let detail = error.details();
            if detail.span.is_none() {
                detail.span = self.span(line, detail.token.as_deref());
                detail.related = detail.related_line.and_then(|line| self.span(line, None));
                error.line = detail.span.as_ref().map(|s| s.line).or(Some(line));
            }
        }
        error
    }
}

fn relative_path(parent: &Path, name: &str) -> Result<PathBuf> {
    let mut path = parent.to_owned();
    if name.is_empty() || name.contains('\\') {
        return Err(Error::new(
            "include needs a relative path using / separators",
        ));
    }
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir if path.pop() => {}
            _ => {
                return Err(Error::new(
                    "include paths must stay within the entry file's directory",
                ));
            }
        }
    }
    Ok(path)
}

#[derive(Default)]
struct Loader {
    base: PathBuf,
    active: HashSet<PathBuf>,
    loaded: BTreeMap<PathBuf, String>,
    bytes: usize,
    lines: usize,
    flat: String,
    map: SourceMap,
}

impl Loader {
    fn file(&mut self, relative: PathBuf) -> Result<()> {
        if self.loaded.contains_key(&relative) {
            return Ok(());
        }
        if self.active.len() >= MAX_INCLUDE_DEPTH
            || self.loaded.len() + self.active.len() >= MAX_FILES
        {
            return Err(Error::new(
                "a composition may include at most 1024 files nested at most 64 levels",
            ));
        }
        if !self.active.insert(relative.clone()) {
            return Err(Error::new(format!(
                "cyclic include of {}",
                relative.display()
            )));
        }
        let path = self.base.join(&relative);
        let name = path.display().to_string();
        let io = |e: std::io::Error| Error::usage(format!("cannot read {name}: {e}"));
        let mut text = String::new();
        std::fs::File::open(&path)
            .map_err(io)?
            .take((MAX_SPEC_BYTES - self.bytes) as u64 + 1)
            .read_to_string(&mut text)
            .map_err(io)?;
        self.bytes += text.len();
        if self.bytes > MAX_SPEC_BYTES {
            return Err(Error::new(format!(
                "source files exceed {MAX_SPEC_BYTES} combined bytes"
            )));
        }
        let hash = fingerprint(&text);
        let mut document = Document::new(name, text, 0);
        let mut body = String::new();
        let mut started = false;
        for (i, line) in document.source.lines().enumerate() {
            let statement = strip_comment(line).trim_end();
            if let Some(arg) = statement
                .strip_prefix("include")
                .filter(|arg| arg.is_empty() || arg.starts_with(char::is_whitespace))
            {
                let include = || -> Result<PathBuf> {
                    if started {
                        return Err(Error::new("put include directives before the first system"));
                    }
                    let value = Json::parse(arg.trim())
                        .map_err(|_| Error::new("include needs a quoted relative file name"))?;
                    let file = value
                        .as_str()
                        .ok_or_else(|| Error::new("include needs a quoted relative file name"))?;
                    relative_path(relative.parent().unwrap_or(Path::new("")), file)
                };
                include()
                    .and_then(|child| self.file(child))
                    .map_err(|mut e| {
                        if e.source_span().is_none() {
                            e.details().span = document.span(i + 1, None);
                            e.line = Some(i + 1);
                        }
                        e
                    })?;
            } else {
                if !started && !statement.trim().is_empty() {
                    if !statement
                        .strip_prefix("system")
                        .is_some_and(|rest| rest.starts_with(char::is_whitespace))
                    {
                        let mut error = Error::at(
                            i + 1,
                            "a source file must begin with a system heading or include directives",
                        );
                        error.details().span = document.span(i + 1, None);
                        return Err(error);
                    }
                    started = true;
                }
                body.push_str(line);
            }
            body.push('\n');
        }
        document.first = self.lines + 1;
        self.lines += document.source.lines().count();
        if !body.is_empty() {
            self.map.0.push(document);
            self.flat.push_str(&body);
        }
        self.active.remove(&relative);
        self.loaded.insert(relative, hash);
        Ok(())
    }
}

/// Load an entry specification with its transitive relative includes. One
/// specification uses at most 4 MiB of source, across all unique files.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Spec> {
    let path = path.as_ref();
    let entry = PathBuf::from(
        path.file_name()
            .ok_or_else(|| Error::usage("expected a specification file"))?,
    );
    let mut loader = Loader {
        base: path.parent().unwrap_or(Path::new("")).to_owned(),
        ..Loader::default()
    };
    loader.file(entry.clone())?;
    let mut spec = parse_flat(&loader.flat).map_err(|e| loader.map.locate(e))?;
    spec.fingerprint = loader.loaded.remove(&entry).expect("loaded entry");
    spec.sources = loader
        .loaded
        .into_iter()
        .map(|(path, hash)| (path.to_string_lossy().replace('\\', "/"), hash))
        .collect();
    spec.source_map = Some(Arc::new(loader.map));
    Ok(spec)
}
