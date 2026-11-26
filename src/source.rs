//! Load a specification and the files it includes, and map parser locations
//! back to the files they came from.
//!
//! Inclusion is textual: each file is read once, its included files come
//! first, and the combined text is parsed as one specification.
use crate::ast::Span;
use crate::parser::{fingerprint, parse_unlocated, strip_comment};
use crate::{Error, Json, Result, Spec};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::io::Read as _;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub(crate) const MAX_SPEC_BYTES: usize = 4 * 1024 * 1024;
const MAX_INCLUDE_DEPTH: usize = 64;
const MAX_FILES: usize = 1024;

/// A located range of source text. Columns count Unicode scalar values from
/// one and `end_column` is exclusive. `file` is empty for text given to
/// [`crate::parse`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SourceMap(Vec<Document>);

/// One file's lines, starting at line `first` of the combined text.
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

    fn text(&self, line: usize) -> Option<&str> {
        let offset = *self.starts.get(line.checked_sub(1)?)?;
        self.source.get(offset..)?.lines().next()
    }

    /// The span of `part`, a slice of line `line`.
    fn span_of(&self, line: usize, part: &str) -> Option<SourceSpan> {
        let text = self.text(line)?;
        let offset = part.as_ptr().addr().checked_sub(text.as_ptr().addr())?;
        let column = 1 + text.get(..offset)?.chars().count();
        Some(SourceSpan {
            file: self.path.clone(),
            line,
            column,
            end_column: column + part.chars().count(),
        })
    }

    /// Where `target` is in `span`, a span of this document: the one
    /// occurrence of `target` inside it as a whole word, or else the one on
    /// its line, since some errors cover only the statement's first name.
    /// Also the text of the line.
    fn find(&self, span: &SourceSpan, target: &str) -> Option<(SourceSpan, String)> {
        let text = self.text(span.line)?;
        let chars: Vec<char> = text.chars().collect();
        let wanted: Vec<char> = target.chars().collect();
        let start = span.column.checked_sub(1)?;
        let end = span.end_column.checked_sub(1)?.min(chars.len());
        let at = match occurrences(&chars, &wanted, start, end).as_slice() {
            [at] => *at,
            [] => match occurrences(&chars, &wanted, 0, chars.len()).as_slice() {
                [at] => *at,
                _ => return None,
            },
            _ => return None,
        };
        Some((
            SourceSpan {
                file: self.path.clone(),
                line: span.line,
                column: at + 1,
                end_column: at + 1 + wanted.len(),
            },
            text.to_owned(),
        ))
    }

    /// The whole statement on `line`, for diagnostics that have no token.
    fn statement(&self, line: usize) -> Option<SourceSpan> {
        let text = self.text(line)?;
        let statement = strip_comment(text).trim_end();
        let indent = statement.len() - statement.trim_start().len();
        let column = 1 + text.get(..indent)?.chars().count();
        Some(SourceSpan {
            file: self.path.clone(),
            line,
            column,
            end_column: column + statement.get(indent..)?.chars().count(),
        })
    }
}

/// Where `wanted` starts in `chars` between `start` and `end`, not counting
/// occurrences that are part of a longer word.
fn occurrences(chars: &[char], wanted: &[char], start: usize, end: usize) -> Vec<usize> {
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    (start..end)
        .filter(|&at| {
            at + wanted.len() <= end
                && chars.get(at..at + wanted.len()) == Some(wanted)
                && !(word(at.checked_sub(1).and_then(|i| chars.get(i))) && word(wanted.first()))
                && !(word(chars.get(at + wanted.len())) && word(wanted.last()))
        })
        .collect()
}

impl SourceMap {
    pub(crate) fn inline(source: &str) -> Arc<Self> {
        Arc::new(Self(vec![Document::new(
            String::new(),
            source.to_owned(),
            1,
        )]))
    }

    /// The document holding combined line `line`, and the line within it.
    fn document(&self, line: usize) -> Option<(&Document, usize)> {
        let index = self
            .0
            .partition_point(|doc| doc.first <= line)
            .checked_sub(1)?;
        let doc = self.0.get(index)?;
        Some((doc, line - doc.first + 1))
    }

    pub(crate) fn resolve(&self, span: Span) -> Option<SourceSpan> {
        let (doc, line) = self.document(span.line)?;
        Some(SourceSpan {
            file: doc.path.clone(),
            line,
            column: span.column,
            end_column: span.end_column,
        })
    }

    pub(crate) fn statement(&self, line: usize) -> Option<SourceSpan> {
        let (doc, line) = self.document(line)?;
        doc.statement(line)
    }

    /// Resolve the parser's location of `error` to its file. Afterwards
    /// `error.line` numbers the line within that file.
    pub(crate) fn locate(&self, error: Error) -> Error {
        if error.source_span().is_some() {
            return error;
        }
        let line = error
            .detail
            .as_ref()
            .and_then(|detail| detail.at)
            .map_or(error.line, |at| Some(at.line));
        let error = self.locate_span(error);
        let (Some((doc, _)), Some(span)) = (
            line.and_then(|line| self.document(line)),
            error.source_span().cloned(),
        ) else {
            return error;
        };
        if doc.path != span.file {
            return error;
        }
        error.resolve_fixes(|target| doc.find(&span, target))
    }

    fn locate_span(&self, mut error: Error) -> Error {
        let (at, related) = match error.detail.as_deref() {
            Some(detail) if detail.source.is_some() => return error,
            Some(detail) => (detail.at, detail.related),
            None => (None, None),
        };
        let span = at
            .and_then(|span| self.resolve(span))
            .or_else(|| error.line.and_then(|line| self.statement(line)));
        let related = related.and_then(|span| self.resolve(span));
        if span.is_none() && related.is_none() {
            return error;
        }
        if let Some(span) = &span {
            error.line = Some(span.line);
        }
        let detail = error.details();
        detail.source = span;
        detail.related_source = related;
        error
    }
}

/// Resolve `name`, a relative include path, against `parent`, a directory
/// relative to the entry file's directory. The result never leaves it.
fn relative_path(parent: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.contains('\\') {
        return Err(Error::new(
            "include needs a relative path using / separators",
        ));
    }
    let mut path = parent.to_owned();
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
    /// For an included file `child` that does not exist, the closest files
    /// next to it, each written as `argument`, the quoted include path,
    /// would write it.
    fn nearest_file(&self, child: &Path, argument: &str) -> Vec<String> {
        let written = Json::parse(argument)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let prefix = written.rsplit_once('/').map_or("", |(dir, _)| dir);
        let (Some(parent), Some(name)) = (child.parent(), child.file_name()) else {
            return Vec::new();
        };
        let Ok(entries) = std::fs::read_dir(self.base.join(parent)) else {
            return Vec::new();
        };
        let names: Vec<String> = entries
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .filter(|file| Path::new(file).extension().is_some_and(|e| e == "zin"))
            .take(MAX_FILES)
            .collect();
        crate::explain::closest(&name.to_string_lossy(), names.iter().map(String::as_str))
            .into_iter()
            .map(|file| {
                let path = if prefix.is_empty() {
                    file
                } else {
                    format!("{prefix}/{file}")
                };
                Json::str(path).to_compact()
            })
            .collect()
    }

    /// Append `relative` and, before it, everything it includes.
    fn file(&mut self, relative: PathBuf) -> Result<()> {
        if self.loaded.contains_key(&relative) {
            return Ok(());
        }
        if self.active.len() >= MAX_INCLUDE_DEPTH
            || self.loaded.len() + self.active.len() >= MAX_FILES
        {
            return Err(Error::new(format!(
                "a specification may include at most {MAX_FILES} files nested at most \
                 {MAX_INCLUDE_DEPTH} levels"
            )));
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
        let remaining = MAX_SPEC_BYTES.saturating_sub(self.bytes);
        std::fs::File::open(&path)
            .map_err(io)?
            .take(
                u64::try_from(remaining)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_to_string(&mut text)
            .map_err(io)?;
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > MAX_SPEC_BYTES {
            return Err(Error::usage(format!(
                "source files exceed {MAX_SPEC_BYTES} combined bytes"
            )));
        }
        let hash = fingerprint(&text);
        let document = Document::new(name, text, 0);
        let mut body = String::with_capacity(document.source.len());
        let mut started = false;
        for (index, line) in document.source.lines().enumerate() {
            let number = index + 1;
            let statement = strip_comment(line).trim();
            if let Some(argument) = statement
                .strip_prefix("include")
                .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
            {
                let argument = argument.trim();
                let child = if started {
                    Err(Error::new("put include directives before the first system"))
                } else {
                    Json::parse(argument)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .ok_or_else(|| Error::new("include needs a quoted relative file name"))
                        .and_then(|file| {
                            relative_path(relative.parent().unwrap_or(Path::new("")), &file)
                        })
                };
                let nearest = child
                    .as_ref()
                    .ok()
                    .filter(|child| !self.base.join(child).is_file())
                    .map(|child| self.nearest_file(child, argument))
                    .unwrap_or_default();
                child.and_then(|child| self.file(child)).map_err(|error| {
                    let span = document
                        .span_of(number, argument)
                        .filter(|_| !argument.is_empty())
                        .or_else(|| document.statement(number));
                    let fixes = span.clone();
                    error
                        .located_at(span)
                        .suggest(argument, nearest)
                        .resolve_fixes(|target| {
                            let span = fixes.as_ref()?;
                            document.find(span, target)
                        })
                })?;
            } else {
                if !started && !statement.is_empty() {
                    if crate::parser::after_keyword(statement, "system").is_none() {
                        return Err(Error::at(
                            number,
                            "a file must begin with include directives or a system heading",
                        )
                        .located_at(document.statement(number)));
                    }
                    started = true;
                }
                body.push_str(line);
            }
            body.push('\n');
        }
        let count = document.source.lines().count();
        self.map.0.push(Document {
            first: self.lines + 1,
            ..document
        });
        self.lines += count;
        self.flat.push_str(&body);
        self.active.remove(&relative);
        self.loaded.insert(relative, hash);
        Ok(())
    }
}

/// Load the specification in file `path` with the files it includes,
/// transitively. Include paths are relative to the including file and stay
/// within the entry file's directory. Each file is read once. The files may
/// hold at most 4 MiB in total.
///
/// # Errors
///
/// An unreadable file, an invalid include, or the first syntax or
/// composition error, located in its file.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Spec> {
    let path = path.as_ref();
    let entry = PathBuf::from(
        path.file_name()
            .ok_or_else(|| Error::usage(format!("{} is not a file", path.display())))?,
    );
    let mut loader = Loader {
        base: path.parent().unwrap_or(Path::new("")).to_owned(),
        ..Loader::default()
    };
    loader.file(entry.clone())?;
    let map = Arc::new(loader.map);
    let mut spec = parse_unlocated(&loader.flat).map_err(|e| map.locate(e))?;
    let entry_hash = loader.loaded.remove(&entry).unwrap_or_default();
    spec.sources = loader
        .loaded
        .into_iter()
        .map(|(path, hash)| (path.to_string_lossy().replace('\\', "/"), hash))
        .collect();
    spec.fingerprint = combined_fingerprint(&entry_hash, &spec.sources);
    spec.source_map = Some(map);
    Ok(spec)
}

/// A single file keeps the fingerprint of its text, so plans saved from it
/// stay valid. With includes, the fingerprint covers every file, so editing
/// any of them invalidates saved plans.
fn combined_fingerprint(entry: &str, sources: &BTreeMap<String, String>) -> String {
    if sources.is_empty() {
        return entry.to_owned();
    }
    let mut text = format!("{entry}\n");
    for (path, hash) in sources {
        let _ = writeln!(text, "{path}\t{hash}");
    }
    fingerprint(&text)
}
