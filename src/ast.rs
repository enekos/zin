use std::collections::BTreeMap;
use std::fmt;

pub const DEFAULT_STATE_LIMIT: usize = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub state_limit: usize,
    pub fingerprint: String,
    pub types: Vec<TypeDecl>,
    pub state: Vec<Field>,
    pub initially: Vec<Assignment>,
    pub properties: Vec<Property>,
    pub terminal: Option<Condition>,
    pub actions: Vec<Action>,
    pub scenarios: Vec<Scenario>,
}

impl Spec {
    #[must_use]
    pub fn empty() -> Self {
        Self {
            name: String::new(),
            state_limit: DEFAULT_STATE_LIMIT,
            fingerprint: String::new(),
            types: Vec::new(),
            state: Vec::new(),
            initially: Vec::new(),
            properties: Vec::new(),
            terminal: None,
            actions: Vec::new(),
            scenarios: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    pub name: String,
    pub kind: TypeExpr,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: TypeExpr,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeExpr {
    Integer { min: i64, max: i64 },
    Enum { values: Vec<String> },
    Boolean,
    Named(String),
    Map { key: String, value: Box<TypeExpr> },
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer { min, max } => write!(formatter, "an integer from {min} to {max}"),
            Self::Enum { values } => write!(formatter, "one of {}", values.join(", ")),
            Self::Boolean => formatter.write_str("a boolean"),
            Self::Named(name) => formatter.write_str(name),
            Self::Map { key, value } => write!(formatter, "a map from {key} to {value}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Condition {
    pub expr: Expr,
    pub source: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyKind {
    Always,
    Never,
    Sometimes,
    Eventually,
    LeadsTo,
    AlwaysPossible,
}

impl PropertyKind {
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
            Self::Sometimes => "sometimes",
            Self::Eventually => "eventually",
            Self::LeadsTo => "whenever",
            Self::AlwaysPossible => "always possible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub kind: PropertyKind,
    pub premise: Option<Condition>,
    pub condition: Condition,
    pub line: usize,
}

impl Property {
    #[must_use]
    pub fn display(&self) -> String {
        match &self.premise {
            Some(premise) => format!(
                "whenever {} eventually {}",
                premise.source, self.condition.source
            ),
            None => format!("{} {}", self.kind.keyword(), self.condition.source),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetKey {
    Whole,
    At(Expr),
    Every(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub field: String,
    pub key: TargetKey,
    pub value: Expr,
    pub source: String,
    pub line: usize,
}

/// What the environment promises about an action's calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Fairness {
    /// No promise: a call may be ignored forever.
    #[default]
    Unfair,
    /// `fair`: a call that stays enabled is eventually taken.
    Weak,
    /// `strongly fair`: a call that is enabled again and again is eventually taken.
    Strong,
}

impl Fairness {
    #[must_use]
    pub fn keyword(self) -> &'static str {
        match self {
            Self::Unfair => "",
            Self::Weak => "fair",
            Self::Strong => "strongly fair",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub name: String,
    pub fairness: Fairness,
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
    Reject(ActionCall),
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

#[must_use]
pub fn display_call(name: &str, arguments: &[Value]) -> String {
    if arguments.is_empty() {
        return name.to_string();
    }
    let arguments = arguments
        .iter()
        .map(Value::source)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({arguments})")
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    Integer(i64),
    Symbol(String),
    Boolean(bool),
    Map(BTreeMap<String, Value>),
}

impl Value {
    #[must_use]
    pub fn source(&self) -> String {
        match self {
            Self::Symbol(value) if is_bare_symbol(value) => value.clone(),
            Self::Symbol(value) => format!(
                "\"{}\"",
                value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t")
            ),
            Self::Map(entries) => {
                let entries = entries
                    .iter()
                    .map(|(key, value)| format!("{key}: {}", value.source()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{{entries}}}")
            }
            Self::Integer(_) | Self::Boolean(_) => self.to_string(),
        }
    }
}

fn is_bare_symbol(value: &str) -> bool {
    !is_reserved_word(value)
        && value
            .chars()
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
}

#[must_use]
pub fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "true"
            | "false"
            | "not"
            | "and"
            | "or"
            | "implies"
            | "is"
            | "equals"
            | "plus"
            | "minus"
            | "times"
            | "divided"
            | "of"
            | "in"
            | "if"
            | "then"
            | "else"
            | "every"
            | "some"
            | "satisfies"
            | "where"
            | "eventually"
    )
}

impl fmt::Display for Value {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::Symbol(value) => formatter.write_str(value),
            Self::Boolean(value) => write!(formatter, "{value}"),
            Self::Map(_) => formatter.write_str(&self.source()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    Literal(Value),
    Name(String),
    Index {
        map: String,
        key: Box<Expr>,
    },
    Unary {
        operator: UnaryOperator,
        value: Box<Expr>,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
    Quantified {
        quantifier: Quantifier,
        variable: String,
        domain: String,
        body: Box<Expr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantifier {
    Every,
    Some,
    Count,
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
