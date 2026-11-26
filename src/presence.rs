//! Reads that fault when a value is absent and that nothing before them
//! guards. A read is guarded when a condition evaluated before it on the same
//! path establishes that the value is there: an earlier `requires` of the
//! action, the left side of `and` or `implies`, the left side of `or` being
//! false, or the condition of an `if`. Facts are matched by the text of the
//! path read, so the analysis needs no state and covers actions that no
//! reachable state enables.

use crate::ast::{BinaryOperator, Expr, Span, Spec, UnaryOperator, Value};
use crate::model::{Absence, Model, PartialRead, describe_expr};
use std::collections::{BTreeMap, HashMap, HashSet};

/// What a value is compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Operand {
    Nothing,
    EmptyList,
    /// A list literal with items.
    Items,
    /// A name or literal, such as an enum value, a parameter or a number.
    Plain(String),
    /// `name with {...}`: a case with members.
    Case(String),
    /// Anything else, which tells nothing about presence.
    Other,
}

#[derive(Debug, Clone)]
enum Claim {
    Equals(Operand),
    Differs(Operand),
    /// The list has an item.
    NotEmpty,
}

#[derive(Debug, Clone)]
struct Fact {
    subject: String,
    claim: Claim,
}

/// The reads in `spec`'s actions, properties and terminal condition that can
/// fault when a value is absent and that nothing guards, in source order.
pub(crate) fn unguarded(spec: &Spec, model: &Model) -> Vec<Unguarded> {
    if model.partial_reads.is_empty() {
        return Vec::new();
    }
    let mut sites: HashMap<Span, Vec<&PartialRead>> = HashMap::new();
    for read in &model.partial_reads {
        sites.entry(read.span).or_default().push(read);
    }
    let mut walk = Walk {
        sites,
        fields: spec.state.iter().map(|field| field.name.as_str()).collect(),
        found: BTreeMap::new(),
    };
    for property in &spec.properties {
        if let Some(premise) = &property.premise {
            walk.visit(&premise.expr, &mut Vec::new());
        }
        walk.visit(&property.condition.expr, &mut Vec::new());
    }
    if let Some(terminal) = &spec.terminal {
        walk.visit(&terminal.expr, &mut Vec::new());
    }
    for action in &spec.actions {
        let mut facts = Vec::new();
        for condition in &action.requires {
            walk.visit(&condition.expr, &mut facts);
            assume(&condition.expr, true, &walk.fields, &mut facts);
        }
        for assignment in &action.assignments {
            walk.visit(&assignment.value, &mut facts);
            walk.visit(&assignment.place, &mut facts);
        }
    }
    walk.found.into_values().collect()
}

/// A read that can find no value and that nothing guards.
pub(crate) struct Unguarded {
    pub span: Span,
    /// The expression that reads, such as `left of lease`.
    pub read: String,
    pub subject: String,
    pub absence: Absence,
}

struct Walk<'a> {
    sites: HashMap<Span, Vec<&'a PartialRead>>,
    /// State fields, which may be absent themselves, so comparing with one
    /// establishes nothing.
    fields: HashSet<&'a str>,
    found: BTreeMap<(usize, usize, String), Unguarded>,
}

impl Walk<'_> {
    fn visit(&mut self, expr: &Expr, facts: &mut Vec<Fact>) {
        match expr {
            Expr::Literal(_) | Expr::Every(_) => {}
            Expr::Name { span, .. } => self.check(expr, *span, facts),
            Expr::Index { key, span, .. } => {
                self.check(expr, *span, facts);
                self.visit(key, facts);
            }
            Expr::Subscript { base, key } => {
                self.visit(base, facts);
                self.visit(key, facts);
            }
            Expr::Record(members) => {
                for (_, value) in members {
                    self.visit(value, facts);
                }
            }
            Expr::Items(items) => {
                for item in items {
                    self.visit(item, facts);
                }
            }
            Expr::Unary { value, .. } => self.visit(value, facts),
            Expr::Binary {
                operator,
                left,
                right,
            } => {
                self.visit(left, facts);
                let mark = facts.len();
                match operator {
                    BinaryOperator::And | BinaryOperator::Implies => {
                        assume(left, true, &self.fields, facts);
                    }
                    BinaryOperator::Or => assume(left, false, &self.fields, facts),
                    _ => {}
                }
                self.visit(right, facts);
                facts.truncate(mark);
            }
            Expr::If {
                condition,
                then,
                otherwise,
            } => {
                self.visit(condition, facts);
                let mark = facts.len();
                assume(condition, true, &self.fields, facts);
                self.visit(then, facts);
                facts.truncate(mark);
                assume(condition, false, &self.fields, facts);
                self.visit(otherwise, facts);
                facts.truncate(mark);
            }
            Expr::Quantified { variable, body, .. } => {
                let mut inner = facts
                    .iter()
                    .filter(|fact| !mentions(&fact.subject, variable))
                    .cloned()
                    .collect();
                self.visit(body, &mut inner);
            }
            Expr::OrElse { default, .. } => self.visit(default, facts),
        }
    }

    fn check(&mut self, expr: &Expr, span: Span, facts: &[Fact]) {
        let Some(reads) = self.sites.get(&span) else {
            return;
        };
        for read in reads {
            if !present(read, facts) {
                self.found
                    .entry((span.line, span.column, read.subject.clone()))
                    .or_insert_with(|| Unguarded {
                        span,
                        read: describe_expr(expr),
                        subject: read.subject.clone(),
                        absence: read.absence.clone(),
                    });
            }
        }
    }
}

/// Whether `facts` establish that the value `read` needs is there.
fn present(read: &PartialRead, facts: &[Fact]) -> bool {
    let about = || facts.iter().filter(|fact| fact.subject == read.subject);
    match &read.absence {
        Absence::Nothing => about().any(|fact| {
            matches!(
                &fact.claim,
                Claim::Differs(Operand::Nothing)
                    | Claim::Equals(Operand::Plain(_) | Operand::Case(_) | Operand::Items)
            )
        }),
        Absence::Empty => about().any(|fact| {
            matches!(
                &fact.claim,
                Claim::NotEmpty
                    | Claim::Equals(Operand::Items)
                    | Claim::Differs(Operand::EmptyList)
            )
        }),
        Absence::Case { holders, others } => {
            about().any(|fact| match &fact.claim {
                Claim::Equals(Operand::Plain(case) | Operand::Case(case)) => holders.contains(case),
                _ => false,
            }) || others.iter().all(|other| {
                about().any(|fact| {
                    matches!(&fact.claim, Claim::Differs(Operand::Plain(case)) if case == other)
                })
            })
        }
    }
}

/// Adds what `expr` evaluating to `holds` establishes.
fn assume(expr: &Expr, holds: bool, fields: &HashSet<&str>, facts: &mut Vec<Fact>) {
    match expr {
        Expr::Unary {
            operator: UnaryOperator::Not,
            value,
        } => assume(value, !holds, fields, facts),
        Expr::Binary {
            operator,
            left,
            right,
        } => match (operator, holds) {
            (BinaryOperator::And, true) | (BinaryOperator::Or, false) => {
                assume(left, holds, fields, facts);
                assume(right, holds, fields, facts);
            }
            (BinaryOperator::Implies, false) => {
                assume(left, true, fields, facts);
                assume(right, false, fields, facts);
            }
            (BinaryOperator::Equal | BinaryOperator::NotEqual, _) => {
                let equal = holds == (*operator == BinaryOperator::Equal);
                compare(left, right, equal, fields, facts);
                compare(right, left, equal, fields, facts);
            }
            (BinaryOperator::Has, true) => {
                if let Some(subject) = path(left) {
                    facts.push(Fact {
                        subject,
                        claim: Claim::NotEmpty,
                    });
                }
            }
            _ => length(*operator, left, right, holds, facts),
        },
        _ => {}
    }
}

/// `subject is other` (or `is not`, when `equal` is false).
fn compare(
    subject: &Expr,
    other: &Expr,
    equal: bool,
    fields: &HashSet<&str>,
    facts: &mut Vec<Fact>,
) {
    let Some(subject) = path(subject) else {
        return;
    };
    let operand = match other {
        Expr::Name { name, .. } if fields.contains(name.as_str()) => Operand::Other,
        other => operand(other),
    };
    if operand == Operand::Other {
        return;
    }
    facts.push(Fact {
        subject,
        claim: if equal {
            Claim::Equals(operand)
        } else {
            Claim::Differs(operand)
        },
    });
}

/// A comparison of `length of l` with a number that rules out 0.
fn length(operator: BinaryOperator, left: &Expr, right: &Expr, holds: bool, facts: &mut Vec<Fact>) {
    let (list, bound, operator) = match (length_of(left), number(right)) {
        (Some(list), Some(bound)) => (list, bound, operator),
        _ => match (number(left), length_of(right)) {
            (Some(bound), Some(list)) => (list, bound, mirror(operator)),
            _ => return,
        },
    };
    let empty_satisfies = match operator {
        BinaryOperator::Equal => bound == 0,
        BinaryOperator::NotEqual => bound != 0,
        BinaryOperator::Less => 0 < bound,
        BinaryOperator::LessEqual => 0 <= bound,
        BinaryOperator::Greater => 0 > bound,
        BinaryOperator::GreaterEqual => 0 >= bound,
        _ => return,
    };
    if empty_satisfies != holds {
        facts.push(Fact {
            subject: list,
            claim: Claim::NotEmpty,
        });
    }
}

fn mirror(operator: BinaryOperator) -> BinaryOperator {
    match operator {
        BinaryOperator::Less => BinaryOperator::Greater,
        BinaryOperator::LessEqual => BinaryOperator::GreaterEqual,
        BinaryOperator::Greater => BinaryOperator::Less,
        BinaryOperator::GreaterEqual => BinaryOperator::LessEqual,
        other => other,
    }
}

fn length_of(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Index { map, key, .. } if map == "length" => path(key),
        _ => None,
    }
}

fn number(expr: &Expr) -> Option<i64> {
    match expr {
        Expr::Literal(Value::Integer(value)) => Some(*value),
        _ => None,
    }
}

/// The text of a path into the state, as reads record their subjects.
fn path(expr: &Expr) -> Option<String> {
    matches!(
        expr,
        Expr::Name { .. } | Expr::Index { .. } | Expr::Subscript { .. }
    )
    .then(|| describe_expr(expr))
}

fn operand(expr: &Expr) -> Operand {
    match expr {
        Expr::Literal(Value::Nothing) => Operand::Nothing,
        Expr::Items(items) if items.is_empty() => Operand::EmptyList,
        Expr::Items(_) => Operand::Items,
        Expr::Name { name, .. } => Operand::Plain(name.clone()),
        Expr::Literal(Value::Symbol(name)) => Operand::Plain(name.clone()),
        Expr::Literal(value @ (Value::Integer(_) | Value::Boolean(_))) => {
            Operand::Plain(value.source())
        }
        Expr::Binary {
            operator: BinaryOperator::With,
            left,
            right,
        } if matches!(right.as_ref(), Expr::Record(_)) => match left.as_ref() {
            Expr::Name { name, .. } | Expr::Literal(Value::Symbol(name)) => {
                Operand::Case(name.clone())
            }
            _ => Operand::Other,
        },
        _ => Operand::Other,
    }
}

/// Whether the path text `subject` names the variable `name`.
fn mentions(subject: &str, name: &str) -> bool {
    subject
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|word| word == name)
}
