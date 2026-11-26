pub mod ast;
mod audit;
mod check;
pub mod cli;
mod component;
mod compose;
pub mod conform;
mod definedness;
mod explain;
mod explore;
mod induction;
mod json;
mod mermaid;
mod model;
mod observe;
mod parser;
mod plan;
mod presence;
mod properties;
mod report;
mod scaffold;
mod source;
mod symmetry;
mod trace;

pub use ast::{Spec, Value};
pub use audit::{Audit, Distinction, Mutant, MutantFate, audit};
pub use check::{
    Options, PropertyResult, ScenarioEvent, ScenarioRun, UnguardedRead, Verification, verify,
    verify_with,
};
pub use component::verify_component;
pub use explain::{Explanation, explanation, explanations};
pub use explore::{Edge, Graph};
pub use induction::{Broken, Induction, InductionCounterexample, check_induction};
pub use json::Json;
pub use mermaid::{sequence_diagram, state_diagram};
pub use model::{ActionInfo, Domain, FieldInfo, FieldType, Model, Param};
pub use observe::{Adequacy, BlindGroup, Hardest, adequacy};
pub use parser::parse;
pub use plan::{Cover, Coverage, Identification, Plan, PlanOptions, PlanStep, PlanTest, TestKind};
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

/// A replacement that fixes an error: `replacement` in place of the text at
/// `span`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fix {
    pub span: SourceSpan,
    pub replacement: String,
    /// What the fix does, such as ``did you mean `pending`?``.
    pub label: String,
}

/// A fix proposed where the error is raised: replace `target`, a name inside
/// the error's span, with `replacement`. Once the error is located, `fix`
/// holds the resolved fix and the source line it changes, unless the target
/// could not be found there.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Suggestion {
    target: String,
    replacement: String,
    label: String,
    located: bool,
    fix: Option<(Fix, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ErrorDetail {
    code: Option<explain::Code>,
    suggestions: Vec<Suggestion>,
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
        use explain::Code;
        self.detail
            .as_ref()
            .and_then(|detail| detail.code)
            .unwrap_or(match self.kind {
                ErrorKind::Spec => Code::Spec,
                ErrorKind::Usage => Code::Usage,
                ErrorKind::Violation => Code::Violation,
                ErrorKind::Model => Code::ModelFault,
                ErrorKind::Limit => Code::Limit,
            })
            .name()
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

    /// Replacements that fix the error, each located in its file. An error
    /// with several fixes offers alternatives; apply one of them.
    pub fn fixes(&self) -> impl Iterator<Item = &Fix> {
        self.detail
            .iter()
            .flat_map(|detail| &detail.suggestions)
            .filter_map(|suggestion| suggestion.fix.as_ref().map(|(fix, _)| fix))
    }

    /// The error as `zin` prints it: the message, then a hint for each
    /// suggested fix showing the corrected line, then where to read more.
    #[must_use]
    pub fn report(&self) -> Report<'_> {
        Report(self)
    }

    fn details(&mut self) -> &mut ErrorDetail {
        self.detail.get_or_insert_with(Box::default)
    }

    pub(crate) fn coded(mut self, code: explain::Code) -> Self {
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

    /// Suggest replacing `target`, which the error's span covers, with each
    /// of `replacements`.
    pub(crate) fn suggest(mut self, target: &str, replacements: Vec<String>) -> Self {
        let suggestions = &mut self.details().suggestions;
        for replacement in replacements {
            suggestions.push(Suggestion {
                target: target.to_owned(),
                label: format!("did you mean `{replacement}`?"),
                replacement,
                located: false,
                fix: None,
            });
        }
        self
    }

    /// Suggest the names among `candidates` closest to `target`.
    pub(crate) fn suggest_closest<'n>(
        self,
        target: &str,
        candidates: impl IntoIterator<Item = &'n str>,
    ) -> Self {
        let closest = explain::closest(target, candidates);
        self.suggest(target, closest)
    }

    /// Resolve pending suggestions with `find`, which locates a target in
    /// the error's file and returns its span and source line.
    pub(crate) fn resolve_fixes(
        mut self,
        find: impl Fn(&str) -> Option<(SourceSpan, String)>,
    ) -> Self {
        if let Some(detail) = self.detail.as_mut() {
            for suggestion in detail.suggestions.iter_mut().filter(|s| !s.located) {
                suggestion.located = true;
                suggestion.fix = find(&suggestion.target).map(|(span, line)| {
                    (
                        Fix {
                            span,
                            replacement: suggestion.replacement.clone(),
                            label: suggestion.label.clone(),
                        },
                        line,
                    )
                });
            }
        }
        self
    }

    /// Whether a code was given where the error was raised, rather than the
    /// default for its kind.
    pub(crate) fn has_specific_code(&self) -> bool {
        self.detail
            .as_ref()
            .is_some_and(|detail| detail.code.is_some())
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

pub(crate) fn display_changes(before: &State, after: &State) -> String {
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

/// An [`Error`] with its hints, as returned by [`Error::report`].
#[derive(Debug, Clone, Copy)]
pub struct Report<'a>(&'a Error);

impl fmt::Display for Report<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let error = self.0;
        write!(formatter, "{error}")?;
        for suggestion in error.detail.iter().flat_map(|detail| &detail.suggestions) {
            write!(formatter, "\n  hint: {}", suggestion.label)?;
            if let Some((fix, line)) = &suggestion.fix {
                write_fixed_line(formatter, fix, line)?;
            }
        }
        if error.has_specific_code() {
            write!(formatter, "\n  = see: zin explain {}", error.code())?;
        }
        Ok(())
    }
}

/// Line `fix.span.line` as it reads once fixed, with the replacement underlined.
fn write_fixed_line(formatter: &mut fmt::Formatter<'_>, fix: &Fix, line: &str) -> fmt::Result {
    let before: String = line
        .chars()
        .take(fix.span.column.saturating_sub(1))
        .collect();
    let after: String = line
        .chars()
        .skip(fix.span.end_column.saturating_sub(1))
        .collect();
    let number = fix.span.line.to_string();
    let gutter = " ".repeat(number.len());
    let indent = " ".repeat(before.chars().count());
    let underline = "^".repeat(fix.replacement.chars().count().max(1));
    write!(
        formatter,
        "\n    {number} | {before}{}{}\n    {gutter} | {indent}{underline}",
        fix.replacement,
        after.trim_end()
    )
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}
