use std::collections::BTreeMap;
use std::fmt;

pub const DEFAULT_STATE_LIMIT: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub state: Vec<Field>,
    pub initially: Vec<Assignment>,
    pub invariants: Vec<Condition>,
    pub actions: Vec<Action>,
    pub scenarios: Vec<Scenario>,
    pub state_limit: usize,
}

impl Spec {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            state: Vec::new(),
            initially: Vec::new(),
            invariants: Vec::new(),
            actions: Vec::new(),
            scenarios: Vec::new(),
            state_limit: DEFAULT_STATE_LIMIT,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Integer { min: i64, max: i64 },
    Enum { values: Vec<String> },
    Boolean,
}

impl fmt::Display for FieldKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer { min, max } => write!(formatter, "an integer from {min} to {max}"),
            Self::Enum { values } => write!(formatter, "one of {}", values.join(", ")),
            Self::Boolean => formatter.write_str("a boolean"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub expr: Expr,
    pub source: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub field: String,
    pub value: Expr,
    pub source: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub params: Vec<Field>,
    pub requires: Vec<Condition>,
    pub assignments: Vec<Assignment>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    pub name: String,
    pub steps: Vec<ScenarioStep>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioStep {
    Call(ActionCall),
    Expect(Condition),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionCall {
    pub name: String,
    pub arguments: CallArguments,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallArguments {
    Positional(Vec<Value>),
    Named(BTreeMap<String, Value>),
}

impl ActionCall {
    #[must_use]
    pub fn display(&self) -> String {
        let arguments = match &self.arguments {
            CallArguments::Positional(values) => values
                .iter()
                .map(Value::source)
                .collect::<Vec<_>>()
                .join(", "),
            CallArguments::Named(values) => values
                .iter()
                .map(|(name, value)| format!("{name} = {}", value.source()))
                .collect::<Vec<_>>()
                .join(", "),
        };
        if arguments.is_empty() {
            self.name.clone()
        } else {
            format!("{}({arguments})", self.name)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    Integer(i64),
    Text(String),
    Boolean(bool),
}

impl Value {
    #[must_use]
    pub fn source(&self) -> String {
        match self {
            Self::Text(value)
                if !value.is_empty()
                    && value != "true"
                    && value != "false"
                    && value
                        .chars()
                        .next()
                        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                    && value
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '_') =>
            {
                value.clone()
            }
            Self::Text(value) => format!(
                "\"{}\"",
                value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t")
            ),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::Text(value) => formatter.write_str(value),
            Self::Boolean(value) => write!(formatter, "{value}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Literal(Value),
    Name(String),
    Unary {
        operator: UnaryOperator,
        value: Box<Expr>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOperator {
    Not,
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOperator {
    Or,
    And,
    Implies,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Add,
    Subtract,
    Multiply,
    Divide,
}
