pub mod ast;
mod audit;
mod check;
pub mod cli;
mod component;
mod compose;
pub mod conform;
mod definedness;
mod explore;
mod json;
mod mermaid;
mod model;
mod parser;
mod plan;
mod properties;
mod report;
mod scaffold;
mod source;
mod symmetry;
mod trace;

pub use ast::{Spec, Value};
pub use audit::{Audit, Distinction, Mutant, MutantFate, audit};
pub use check::{
    Options, PropertyResult, ScenarioEvent, ScenarioRun, Verification, verify, verify_with,
};
pub use component::verify_component;
pub use explore::{Edge, Graph};
pub use json::Json;
pub use mermaid::{sequence_diagram, state_diagram};
pub use model::{ActionInfo, Domain, FieldInfo, FieldType, Model, Param};
pub use parser::parse;
pub use plan::{Cover, Coverage, Plan, PlanOptions, PlanStep, PlanTest, TestKind};
pub use report::render_markdown;
pub use scaffold::{Framework, scaffold};
pub use source::{SourceSpan, parse_file};
pub use trace::{TraceReport, check_trace, check_trace_reader};

use std::collections::BTreeMap;
use std::fmt;

pub type State = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Usage,
    Spec,
    Model,
    Violation,
    Limit,
}

impl ErrorKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Spec => "spec",
            Self::Model => "model",
            Self::Violation => "violation",
            Self::Limit => "limit",
        }
    }

    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Model | Self::Violation => 1,
            Self::Usage | Self::Spec | Self::Limit => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceStep {
    pub call: String,
    pub state: State,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub initial: State,
    pub steps: Vec<TraceStep>,
    pub loop_from: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// The line of the specification the error refers to. Once the error
    /// leaves `parse`, `parse_file`, `Model::compile` or `verify`, this is the
    /// line within [`Error::source_span`]'s file.
    pub line: Option<usize>,
    pub message: String,
    pub trace: Option<Trace>,
    detail: Option<Box<ErrorDetail>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ErrorDetail {
    code: Option<&'static str>,
    /// Where the parser found the problem, in combined-text coordinates.
    at: Option<ast::Span>,
    related: Option<ast::Span>,
    /// `at` and `related` resolved to their files.
    source: Option<SourceSpan>,
    related_source: Option<SourceSpan>,
}

impl Error {
    /// A stable identifier for the kind of diagnostic, such as `name.unknown`.
    #[must_use]
    pub fn code(&self) -> &'static str {
        self.detail
            .as_ref()
            .and_then(|detail| detail.code)
            .unwrap_or(match self.kind {
                ErrorKind::Spec => "spec",
                ErrorKind::Usage => "usage",
                ErrorKind::Violation => "violation",
                ErrorKind::Model => "model.fault",
                ErrorKind::Limit => "limit",
            })
    }

    /// The token or statement the error refers to.
    #[must_use]
    pub fn source_span(&self) -> Option<&SourceSpan> {
        self.detail.as_ref()?.source.as_ref()
    }

    /// A second location, such as the first of two conflicting declarations.
    #[must_use]
    pub fn related_span(&self) -> Option<&SourceSpan> {
        self.detail.as_ref()?.related_source.as_ref()
    }

    fn details(&mut self) -> &mut ErrorDetail {
        self.detail.get_or_insert_with(Box::default)
    }

    pub(crate) fn coded(mut self, code: &'static str) -> Self {
        self.details().code = Some(code);
        self
    }

    /// Locate the error at `span`, which the parser recorded.
    pub(crate) fn spanned(mut self, span: ast::Span) -> Self {
        self.line = Some(span.line);
        self.details().at = Some(span);
        self
    }

    /// Locate the error at `span` unless it already has a more precise
    /// location or refers to another line.
    pub(crate) fn within(self, span: ast::Span) -> Self {
        let located = self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.at.is_some());
        if located || self.line.is_some_and(|line| line != span.line) {
            self
        } else {
            self.spanned(span)
        }
    }

    /// Locate an error found outside the parser directly in its file.
    pub(crate) fn located_at(mut self, span: Option<SourceSpan>) -> Self {
        let located = self
            .detail
            .as_ref()
            .is_some_and(|detail| detail.source.is_some());
        if let Some(span) = span.filter(|_| !located) {
            self.line = Some(span.line);
            self.details().source = Some(span);
        }
        self
    }

    pub(crate) fn related(mut self, span: ast::Span) -> Self {
        self.details().related = Some(span);
        self
    }

    pub fn new(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Spec, None, message)
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Usage, None, message)
    }

    pub fn at(line: usize, message: impl Into<String>) -> Self {
        Self::of(ErrorKind::Spec, Some(line), message)
    }

    pub fn of(kind: ErrorKind, line: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            kind,
            line,
            message: message.into(),
            trace: None,
            detail: None,
        }
    }

    #[must_use]
    pub fn with_trace(mut self, trace: Trace) -> Self {
        self.trace = Some(trace);
        self
    }
}

#[must_use]
pub fn display_state(state: &State) -> String {
    state
        .iter()
        .map(|(name, value)| format!("{name}={}", value.source()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn display_changes(before: &State, after: &State) -> String {
    let changes = after
        .iter()
        .filter(|(name, value)| before.get(*name) != Some(value))
        .map(|(name, value)| format!("{name}={}", value.source()))
        .collect::<Vec<_>>();
    if changes.is_empty() {
        "(no change)".to_string()
    } else {
        changes.join(", ")
    }
}

impl fmt::Display for Trace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "  start: {}", display_state(&self.initial))?;
        let mut before = &self.initial;
        for (index, step) in self.steps.iter().enumerate() {
            if self.loop_from == Some(index) {
                write!(formatter, "\n  then forever:")?;
            }
            write!(
                formatter,
                "\n  {}. {}  ->  {}",
                index + 1,
                step.call,
                display_changes(before, &step.state)
            )?;
            before = &step.state;
        }
        Ok(())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.source_span(), self.line) {
            (Some(span), _) if !span.file.is_empty() => {
                write!(formatter, "{}:{}:{}: ", span.file, span.line, span.column)?;
            }
            (_, Some(line)) => write!(formatter, "line {line}: ")?,
            _ => {}
        }
        formatter.write_str(&self.message)?;
        if let Some(span) = self.related_span() {
            if span.file.is_empty() {
                write!(formatter, " (first declared on line {})", span.line)?;
            } else {
                write!(
                    formatter,
                    " (first declared at {}:{}:{})",
                    span.file, span.line, span.column
                )?;
            }
        }
        if let Some(trace) = &self.trace {
            write!(formatter, "\ncounterexample:\n{trace}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
