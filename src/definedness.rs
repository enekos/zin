//! Which expressions can fault, judged by interval analysis over declared domains.
//!
//! Evaluation is the same up to renaming of interchangeable values, except
//! where a short-circuiting quantifier visits those values in raw order: if
//! its body can fault for one value and decide the result for another, the
//! representative and a concrete state can disagree on whether a fault
//! happens. Symmetry reduction is sound only when no such quantifier exists.

use crate::ast::{BinaryOperator, Quantifier};
use crate::model::{Domain, Ir, Model, Target};

/// The values an expression can take.
#[derive(Debug, Clone)]
enum Values {
    Range(i128, i128),
    /// Raw enum values, sorted.
    Raws(Vec<i64>),
}

const ANY: Values = Values::Range(i64::MIN as i128, i64::MAX as i128);
const BOOLEAN: Values = Values::Range(0, 1);

impl Values {
    fn of(domain: &Domain) -> Self {
        match domain {
            Domain::Integer { min, max } => Self::Range(i128::from(*min), i128::from(*max)),
            Domain::Boolean => BOOLEAN,
            Domain::Enum(ids) => {
                let mut raws: Vec<i64> = ids.iter().map(|id| i64::from(*id)).collect();
                raws.sort_unstable();
                Self::Raws(raws)
            }
        }
    }

    fn bounds(&self) -> (i128, i128) {
        match self {
            Self::Range(min, max) => (*min, *max),
            Self::Raws(raws) => (
                raws.first().map_or(0, |raw| i128::from(*raw)),
                raws.last().map_or(0, |raw| i128::from(*raw)),
            ),
        }
    }

    fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::Raws(mut a), Self::Raws(b)) => {
                a.extend(b);
                a.sort_unstable();
                a.dedup();
                Self::Raws(a)
            }
            (a, b) => {
                let ((a_min, a_max), (b_min, b_max)) = (a.bounds(), b.bounds());
                Self::Range(a_min.min(b_min), a_max.max(b_max))
            }
        }
    }

    /// Whether every value is a key of `keys`.
    fn within(&self, keys: &Domain) -> bool {
        match (self, keys) {
            (Self::Raws(raws), _) => raws.iter().all(|raw| keys.position(*raw).is_some()),
            (
                Self::Range(min, max),
                Domain::Integer {
                    min: low,
                    max: high,
                },
            ) => *min >= i128::from(*low) && *max <= i128::from(*high),
            (Self::Range(min, max), Domain::Boolean) => *min >= 0 && *max <= 1,
            (Self::Range(min, max), Domain::Enum(ids)) => {
                max - min < 64
                    && (*min..=*max).all(|raw| {
                        i64::try_from(raw)
                            .is_ok_and(|raw| ids.iter().any(|id| i64::from(*id) == raw))
                    })
            }
        }
    }
}

fn fits(min: i128, max: i128) -> bool {
    min >= i128::from(i64::MIN) && max <= i128::from(i64::MAX)
}

fn clamp(min: i128, max: i128) -> Values {
    let (low, high) = (i128::from(i64::MIN), i128::from(i64::MAX));
    Values::Range(min.clamp(low, high), max.clamp(low, high))
}

/// What the analysis learns about one expression.
struct Facts {
    values: Values,
    /// Evaluation can fail for some state the declarations allow.
    can_fault: bool,
    /// Whether evaluation faults depends on the order of interchangeable values.
    order_sensitive: bool,
}

impl Facts {
    fn total(values: Values) -> Self {
        Self {
            values,
            can_fault: false,
            order_sensitive: false,
        }
    }
}

struct Analysis<'a> {
    model: &'a Model,
    /// What each local can hold; `None` means unknown.
    locals: Vec<Option<Values>>,
}

impl Analysis<'_> {
    fn interchangeable(&self, raw: i64) -> bool {
        self.model
            .interchangeable
            .iter()
            .any(|group| group.contains(&raw))
    }

    /// Every slot an index can reach has the domain of the first one.
    fn slot_values(&self, slot: usize) -> Values {
        self.model
            .slots
            .get(slot)
            .map_or(ANY, |slot| Values::of(&slot.domain))
    }

    fn facts(&mut self, ir: &Ir) -> Facts {
        match ir {
            Ir::Const(raw) => Facts::total(Values::Range(i128::from(*raw), i128::from(*raw))),
            Ir::Slot(slot) => Facts::total(self.slot_values(*slot)),
            Ir::Local(local) => {
                Facts::total(self.locals.get(*local).cloned().flatten().unwrap_or(ANY))
            }
            Ir::Index { offset, steps } => {
                let mut facts = Facts::total(self.slot_values(*offset));
                for step in steps {
                    let key = self.facts(&step.key);
                    facts.can_fault |= key.can_fault || !key.values.within(&step.keys);
                    facts.order_sensitive |= key.order_sensitive;
                }
                facts
            }
            Ir::Tuple(parts) => {
                let mut facts = Facts::total(BOOLEAN);
                for part in parts {
                    let part = self.facts(part);
                    facts.values = facts.values.union(part.values);
                    facts.can_fault |= part.can_fault;
                    facts.order_sensitive |= part.order_sensitive;
                }
                facts
            }
            Ir::Not(value) => {
                let value = self.facts(value);
                Facts {
                    values: BOOLEAN,
                    ..value
                }
            }
            Ir::Negate(value) => {
                let value = self.facts(value);
                let (min, max) = value.values.bounds();
                Facts {
                    values: clamp(-max, -min),
                    can_fault: value.can_fault || !fits(-max, -min),
                    order_sensitive: value.order_sensitive,
                }
            }
            Ir::If(condition, then, otherwise) => {
                let (condition, then, otherwise) = (
                    self.facts(condition),
                    self.facts(then),
                    self.facts(otherwise),
                );
                Facts {
                    values: then.values.union(otherwise.values),
                    can_fault: condition.can_fault || then.can_fault || otherwise.can_fault,
                    order_sensitive: condition.order_sensitive
                        || then.order_sensitive
                        || otherwise.order_sensitive,
                }
            }
            Ir::Binary(operator, left, right) => {
                let (left, right) = (self.facts(left), self.facts(right));
                let (values, partial) = arithmetic(*operator, &left.values, &right.values);
                Facts {
                    values,
                    can_fault: left.can_fault || right.can_fault || partial,
                    order_sensitive: left.order_sensitive || right.order_sensitive,
                }
            }
            Ir::Quantified {
                quantifier,
                local,
                values,
                body,
            } => {
                let mut raws = values.clone();
                raws.sort_unstable();
                raws.dedup();
                let visits_interchangeable = raws.iter().any(|raw| self.interchangeable(*raw));
                let saved = self.bind(*local, Some(Values::Raws(raws)));
                let body = self.facts(body);
                self.bind(*local, saved);
                let count = i128::try_from(values.len()).unwrap_or(i128::MAX);
                Facts {
                    values: if *quantifier == Quantifier::Count {
                        Values::Range(0, count)
                    } else {
                        BOOLEAN
                    },
                    can_fault: !values.is_empty() && body.can_fault,
                    order_sensitive: body.order_sensitive
                        || (*quantifier != Quantifier::Count
                            && visits_interchangeable
                            && body.can_fault),
                }
            }
        }
    }

    fn bind(&mut self, local: usize, values: Option<Values>) -> Option<Values> {
        if self.locals.len() <= local {
            self.locals.resize(local + 1, None);
        }
        std::mem::replace(&mut self.locals[local], values)
    }

    fn order_sensitive(&mut self, ir: &Ir) -> bool {
        self.facts(ir).order_sensitive
    }
}

/// The result range of `operator` and whether it can fault.
fn arithmetic(operator: BinaryOperator, left: &Values, right: &Values) -> (Values, bool) {
    let ((a, b), (c, d)) = (left.bounds(), right.bounds());
    let corners = |f: fn(i128, i128) -> i128| {
        let all = [f(a, c), f(a, d), f(b, c), f(b, d)];
        (
            all.into_iter().min().unwrap_or(0),
            all.into_iter().max().unwrap_or(0),
        )
    };
    let (min, max) = match operator {
        BinaryOperator::Add => (a + c, b + d),
        BinaryOperator::Subtract => (a - d, b - c),
        BinaryOperator::Multiply => corners(|x, y| x.saturating_mul(y)),
        BinaryOperator::Divide => {
            if c <= 0 && 0 <= d {
                let largest = a.abs().max(b.abs());
                return (clamp(-largest, largest), true);
            }
            corners(|x, y| x / y)
        }
        _ => return (BOOLEAN, false),
    };
    (clamp(min, max), !fits(min, max))
}

/// Whether some short-circuiting quantifier over interchangeable values can
/// fault for one value and stop early for another.
pub(crate) fn order_sensitive(model: &Model) -> bool {
    let mut analysis = Analysis {
        model,
        locals: Vec::new(),
    };
    let global = model.properties.iter().any(|property| {
        analysis.order_sensitive(&property.condition.ir)
            || property
                .premise
                .as_ref()
                .is_some_and(|premise| analysis.order_sensitive(&premise.ir))
    }) || model
        .terminal
        .as_ref()
        .is_some_and(|terminal| analysis.order_sensitive(&terminal.ir));
    global
        || model.actions.iter().any(|action| {
            let mut analysis = Analysis {
                model,
                locals: action
                    .params
                    .iter()
                    .map(|param| Some(Values::of(&param.domain)))
                    .collect(),
            };
            action
                .requires
                .iter()
                .any(|condition| analysis.order_sensitive(&condition.ir))
                || action.assignments.iter().any(|assignment| {
                    analysis.order_sensitive(&assignment.value)
                        || matches!(&assignment.target, Target::Indexed { steps, .. }
                            if steps.iter().any(|step| analysis.order_sensitive(&step.key)))
                })
        })
}

#[cfg(test)]
mod tests {
    use super::{Values, arithmetic};
    use crate::ast::BinaryOperator;

    #[test]
    fn bounded_arithmetic_cannot_fault() {
        let small = Values::Range(0, 5);
        assert!(!arithmetic(BinaryOperator::Add, &small, &small).1);
        assert!(!arithmetic(BinaryOperator::Multiply, &small, &small).1);
        assert!(!arithmetic(BinaryOperator::Divide, &small, &Values::Range(1, 3)).1);
    }

    #[test]
    fn possible_faults_are_reported() {
        let any = Values::Range(i128::from(i64::MIN), i128::from(i64::MAX));
        assert!(arithmetic(BinaryOperator::Add, &any, &Values::Range(1, 1)).1);
        assert!(
            arithmetic(
                BinaryOperator::Divide,
                &Values::Range(1, 1),
                &Values::Range(-1, 1)
            )
            .1
        );
        assert!(arithmetic(BinaryOperator::Divide, &any, &Values::Range(-1, -1)).1);
        let (values, _) = arithmetic(
            BinaryOperator::Divide,
            &Values::Range(-7, 9),
            &Values::Range(2, 3),
        );
        assert_eq!(values.bounds(), (-3, 4));
    }
}
