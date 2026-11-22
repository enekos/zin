use crate::ast::{BinaryOperator, Expr, Spec, Value};
use crate::check::{Verification, verify};
use crate::{ErrorKind, Result};
use std::ops::ControlFlow;

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
    /// Shortest common prefix followed by the first call whose outcomes differ.
    pub witness: Option<Distinction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Distinction {
    pub prefix: Vec<String>,
    pub call: String,
    /// None denotes a refusal, Some contains the state after acceptance.
    pub original: Option<crate::State>,
    pub mutated: Option<crate::State>,
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
        Expr::Literal(_) | Expr::Name(_) => Vec::new(),
        Expr::Index { map, key } => boxed(variants(key), |key| Expr::Index {
            map: map.clone(),
            key,
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

fn visit_mutants(spec: &Spec, mut visit: impl FnMut(usize, String, &Spec)) -> ControlFlow<()> {
    let mut candidate = spec.clone();
    let mut tried = 0;
    for (index, action) in spec.actions.iter().enumerate() {
        let mut push = |line, description: String, edit: &dyn Fn(&mut crate::ast::Action)| {
            if tried == MAX_MUTANTS {
                return ControlFlow::Break(());
            }
            tried += 1;
            candidate.actions[index].clone_from(action);
            edit(&mut candidate.actions[index]);
            visit(
                line,
                format!("action {}: {description}", action.name),
                &candidate,
            );
            ControlFlow::Continue(())
        };
        for (position, condition) in action.requires.iter().enumerate() {
            push(
                condition.line,
                format!("drop 'requires {}'", condition.source),
                &|action| {
                    action.requires.remove(position);
                },
            )?;
            for (description, expr) in variants(&condition.expr) {
                push(
                    condition.line,
                    format!("in 'requires {}', {description}", condition.source),
                    &|action| {
                        if let Some(condition) = action.requires.get_mut(position) {
                            condition.expr = expr.clone();
                        }
                    },
                )?;
            }
        }
        for (position, assignment) in action.assignments.iter().enumerate() {
            push(
                assignment.line,
                format!("drop '{}'", assignment.source),
                &|action| {
                    action.assignments.remove(position);
                },
            )?;
            for (description, expr) in variants(&assignment.value) {
                push(
                    assignment.line,
                    format!("in '{}', {description}", assignment.source),
                    &|action| {
                        if let Some(assignment) = action.assignments.get_mut(position) {
                            assignment.value = expr.clone();
                        }
                    },
                )?;
            }
        }
        candidate.actions[index].clone_from(action);
    }
    ControlFlow::Continue(())
}

// Parent links are BFS ordered. Before the first difference, a prefix reaches
// the same concrete state in both machines, even if the numeric state IDs differ.
fn distinguish(original: &Verification, mutated: &Verification) -> Option<Distinction> {
    let left = &original.graph;
    let right = &mutated.graph;
    if left.first_edge == right.first_edge
        && left.states == right.states
        && left.edges == right.edges
    {
        return None;
    }
    for (id, state) in left.states.iter().enumerate() {
        let Some(&other) = right.index.get(state) else {
            continue;
        };
        let from = crate::model::index32(id);
        let mut a = left.out(from).iter().peekable();
        let mut b = right.out(other).iter().peekable();
        while let Some(call) = [a.peek().map(|e| e.call), b.peek().map(|e| e.call)]
            .into_iter()
            .flatten()
            .min()
        {
            let to = a.next_if(|e| e.call == call).map(|e| e.to);
            let changed = b.next_if(|e| e.call == call).map(|e| e.to);
            if to.map(|v| left.state(v)) != changed.map(|v| right.state(v)) {
                return Some(Distinction {
                    prefix: left
                        .path_to(from)
                        .iter()
                        .map(|e| original.call_text(e.call))
                        .collect(),
                    call: original.call_text(call),
                    original: to.map(|v| original.state(v)),
                    mutated: changed.map(|v| mutated.state(v)),
                });
            }
        }
    }
    None
}

/// Mutation-tests the specification itself and records a concrete witness for each survivor.
pub fn audit(spec: &Spec) -> Result<Audit> {
    let original = verify(spec)?;
    let mut mutants = Vec::new();
    let truncated = visit_mutants(spec, |line, description, candidate| {
        let mut witness = None;
        let fate = match verify(candidate) {
            Ok(mutated) => {
                witness = distinguish(&original, &mutated);
                if witness.is_some() {
                    MutantFate::Survived
                } else {
                    MutantFate::Equivalent
                }
            }
            Err(error) => match error.kind {
                ErrorKind::Violation | ErrorKind::Model => {
                    MutantFate::Caught(error.message.lines().next().unwrap_or("").to_owned())
                }
                ErrorKind::Limit => MutantFate::Inconclusive,
                ErrorKind::Spec | ErrorKind::Usage => MutantFate::Invalid,
            },
        };
        mutants.push(Mutant {
            line,
            description,
            fate,
            witness,
        });
    })
    .is_break();
    Ok(Audit { mutants, truncated })
}
