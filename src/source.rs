//! Map parser locations back to the files they came from.
use crate::Error;
use crate::ast::Span;
use crate::parser::strip_comment;
use std::sync::Arc;

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
    pub(crate) fn locate(&self, mut error: Error) -> Error {
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
