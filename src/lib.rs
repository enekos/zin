mod analysis;
mod ast;
mod check;
mod mermaid;
mod parser;
mod report;

pub use ast::{
    Action, ActionCall, Assignment, BinaryOperator, CallArguments, Condition, Expr, Field,
    FieldKind, Scenario, ScenarioStep, Spec, UnaryOperator, Value,
};
pub use check::{ScenarioResult, State, Transition, Verification, verify};
pub use mermaid::{sequence_diagram, state_diagram};
pub use parser::parse;
pub use report::render_markdown;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub line: Option<usize>,
    pub message: String,
}

impl Error {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            line: None,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn at(line: usize, message: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(line) = self.line {
            write!(formatter, "line {line}: {}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
