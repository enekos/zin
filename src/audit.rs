use crate::ast::{BinaryOperator, Expr, Spec, Value};
use crate::check::verify;
use crate::{ErrorKind, Result};

const MAX_MUTANTS: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutantFate {
    /// A property, scenario, or declared range notices the change.
    Caught(String),
    /// Behavior changes and nothing in the specification notices.
    Survived,
    /// The change does not alter the reachable behavior.
    Equivalent,
    /// The change does not type-check.
    Invalid,
    /// The mutant exceeded an exploration limit.
    Inconclusive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mutant {
    pub line: usize,
    pub description: String,
    pub fate: MutantFate,
}

#[derive(Debug, Clone)]
pub struct Audit {
    pub mutants: Vec<Mutant>,
    pub truncated: bool,
}

impl Audit {
    fn count(&self, wanted: impl Fn(&MutantFate) -> bool) -> usize {
        self.mutants
            .iter()
            .filter(|mutant| wanted(&mutant.fate))
            .count()
    }

    #[must_use]
    pub fn caught(&self) -> usize {
        self.count(|fate| matches!(fate, MutantFate::Caught(_)))
    }

    #[must_use]
    pub fn survivors(&self) -> Vec<&Mutant> {
        self.mutants
            .iter()
            .filter(|mutant| mutant.fate == MutantFate::Survived)
            .collect()
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let survived = self.survivors().len();
        let judged = self.caught() + survived;
        format!(
            "{judged} behavior-changing mutants: {} caught, {survived} survived ({} equivalent, {} skipped)",
            self.caught(),
            self.count(|fate| *fate == MutantFate::Equivalent),
            self.count(|fate| matches!(fate, MutantFate::Invalid | MutantFate::Inconclusive)),
        )
    }
}

fn swapped(operator: BinaryOperator) -> Option<(BinaryOperator, &'static str)> {
    Some(match operator {
        BinaryOperator::Less => (BinaryOperator::LessEqual, "'<' becomes '<='"),
        BinaryOperator::LessEqual => (BinaryOperator::Less, "'<=' becomes '<'"),
        BinaryOperator::Greater => (BinaryOperator::GreaterEqual, "'>' becomes '>='"),
        BinaryOperator::GreaterEqual => (BinaryOperator::Greater, "'>=' becomes '>'"),
        BinaryOperator::Equal => (BinaryOperator::NotEqual, "'is' becomes 'is not'"),
        BinaryOperator::NotEqual => (BinaryOperator::Equal, "'is not' becomes 'is'"),
        BinaryOperator::And => (BinaryOperator::Or, "'and' becomes 'or'"),
        BinaryOperator::Or => (BinaryOperator::And, "'or' becomes 'and'"),
        BinaryOperator::Add => (BinaryOperator::Subtract, "'+' becomes '-'"),
        BinaryOperator::Subtract => (BinaryOperator::Add, "'-' becomes '+'"),
        BinaryOperator::Implies | BinaryOperator::Multiply | BinaryOperator::Divide => return None,
    })
}

fn boxed(variants: Vec<(String, Expr)>, wrap: impl Fn(Box<Expr>) -> Expr) -> Vec<(String, Expr)> {
    variants
        .into_iter()
        .map(|(description, expr)| (description, wrap(Box::new(expr))))
        .collect()
}

fn variants(expr: &Expr) -> Vec<(String, Expr)> {
    match expr {
        Expr::Literal(Value::Integer(raw)) => [raw.checked_add(1), raw.checked_sub(1)]
            .into_iter()
            .flatten()
            .map(|changed| {
                (
                    format!("{raw} becomes {changed}"),
                    Expr::Literal(Value::Integer(changed)),
                )
            })
            .collect(),
        Expr::Literal(_) | Expr::Name { .. } => Vec::new(),
        Expr::Index { map, key, span } => boxed(variants(key), |key| Expr::Index {
            map: map.clone(),
            key,
            span: *span,
        }),
        Expr::Unary { operator, value } => boxed(variants(value), |value| Expr::Unary {
            operator: *operator,
            value,
        }),
        Expr::Binary {
            operator,
            left,
            right,
        } => {
            let mut all = Vec::new();
            if let Some((other, description)) = swapped(*operator) {
                all.push((
                    description.to_string(),
                    Expr::Binary {
                        operator: other,
                        left: left.clone(),
                        right: right.clone(),
                    },
                ));
            }
            all.extend(boxed(variants(left), |left| Expr::Binary {
                operator: *operator,
                left,
                right: right.clone(),
            }));
            all.extend(boxed(variants(right), |right| Expr::Binary {
                operator: *operator,
                left: left.clone(),
                right,
            }));
            all
        }
        Expr::If {
            condition,
            then,
            otherwise,
        } => {
            let mut all = boxed(variants(condition), |condition| Expr::If {
                condition,
                then: then.clone(),
                otherwise: otherwise.clone(),
            });
            all.extend(boxed(variants(then), |then| Expr::If {
                condition: condition.clone(),
                then,
                otherwise: otherwise.clone(),
            }));
            all.extend(boxed(variants(otherwise), |otherwise| Expr::If {
                condition: condition.clone(),
                then: then.clone(),
                otherwise,
            }));
            all
        }
        Expr::Quantified {
            quantifier,
            variable,
            domain,
            body,
        } => boxed(variants(body), |body| Expr::Quantified {
            quantifier: *quantifier,
            variable: variable.clone(),
            domain: domain.clone(),
            body,
        }),
    }
}

struct Candidate {
    line: usize,
    description: String,
    spec: Spec,
}

fn candidates(spec: &Spec) -> Vec<Candidate> {
    let mut all = Vec::new();
    for (index, action) in spec.actions.iter().enumerate() {
        let mut push =
            |line: usize, description: String, edit: &dyn Fn(&mut crate::ast::Action)| {
                let mut mutated = spec.clone();
                if let Some(action) = mutated.actions.get_mut(index) {
                    edit(action);
                }
                all.push(Candidate {
                    line,
                    description: format!("action {}: {description}", action.name),
                    spec: mutated,
                });
            };
        for (position, condition) in action.requires.iter().enumerate() {
            push(
                condition.line,
                format!("drop 'requires {}'", condition.source),
                &|action| {
                    action.requires.remove(position);
                },
            );
            for (description, expr) in variants(&condition.expr) {
                push(
                    condition.line,
                    format!("in 'requires {}', {description}", condition.source),
                    &|action| {
                        if let Some(condition) = action.requires.get_mut(position) {
                            condition.expr = expr.clone();
                        }
                    },
                );
            }
        }
        for (position, assignment) in action.assignments.iter().enumerate() {
            push(
                assignment.line,
                format!("drop '{}'", assignment.source),
                &|action| {
                    action.assignments.remove(position);
                },
            );
            for (description, expr) in variants(&assignment.value) {
                push(
                    assignment.line,
                    format!("in '{}', {description}", assignment.source),
                    &|action| {
                        if let Some(assignment) = action.assignments.get_mut(position) {
                            assignment.value = expr.clone();
                        }
                    },
                );
            }
        }
    }
    all
}

/// Mutation-tests the specification itself: each small change to an action
/// should be noticed by some property, scenario, or declared range.
pub fn audit(spec: &Spec) -> Result<Audit> {
    let original = verify(spec)?;
    let mut candidates = candidates(spec);
    let truncated = candidates.len() > MAX_MUTANTS;
    candidates.truncate(MAX_MUTANTS);
    let mutants = candidates
        .into_iter()
        .map(|candidate| {
            let fate = match verify(&candidate.spec) {
                Ok(mutated) => {
                    if mutated.graph.states == original.graph.states
                        && mutated.graph.edges == original.graph.edges
                    {
                        MutantFate::Equivalent
                    } else {
                        MutantFate::Survived
                    }
                }
                Err(error) => match error.kind {
                    ErrorKind::Violation | ErrorKind::Model => {
                        let first = error.message.lines().next().unwrap_or("").to_string();
                        MutantFate::Caught(first)
                    }
                    ErrorKind::Limit => MutantFate::Inconclusive,
                    ErrorKind::Spec | ErrorKind::Usage => MutantFate::Invalid,
                },
            };
            Mutant {
                line: candidate.line,
                description: candidate.description,
                fate,
            }
        })
        .collect();
    Ok(Audit { mutants, truncated })
}
