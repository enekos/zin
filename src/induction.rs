//! Whether the `always` and `never` properties together are an inductive
//! invariant: every call from any well-typed state that satisfies them,
//! reachable or not, leads to a state that satisfies them again.

use crate::ast::PropertyKind;
use crate::check::Verification;
use crate::explore::MAX_TRANSITIONS;
use crate::model::{Fault, FieldType, Model, index32};
use crate::{Error, ErrorKind, Result, State};

/// The result of [`check_induction`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Induction {
    /// How many `always` and `never` properties make up the invariant.
    pub properties: usize,
    /// Every state the field types allow, reachable or not.
    pub well_typed_states: usize,
    /// The well-typed states in which every property holds.
    pub satisfying_states: usize,
    /// The first call, in enumeration order, that leads out of the invariant.
    pub counterexample: Option<InductionCounterexample>,
}

impl Induction {
    /// Whether every call preserves the properties.
    #[must_use]
    pub fn holds(&self) -> bool {
        self.counterexample.is_none()
    }
}

/// A state that satisfies every `always` and `never` property and a call
/// that leaves it for a state that does not, or that faults. Such a state is
/// never reachable, since the reachable states all passed the same check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InductionCounterexample {
    pub state: State,
    pub call: String,
    /// The state the call leads to; `None` when taking the call faults.
    pub next: Option<State>,
    pub broken: Broken,
}

/// What a counterexample to induction breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Broken {
    /// The property, as written, and its line.
    Property { text: String, line: usize },
    /// A model fault while taking the call or evaluating a property after it.
    Fault { message: String, line: usize },
}

/// Checks whether the `always` and `never` properties of a verified
/// specification are inductive. A state in which evaluating a property
/// faults does not satisfy the properties.
///
/// # Errors
///
/// A limit error when the field types allow more states than the
/// specification's state limit, or the satisfying states take more calls
/// than exploration would.
pub fn check_induction(verification: &Verification) -> Result<Induction> {
    let model = &verification.model;
    let properties = model
        .properties
        .iter()
        .filter(|property| matches!(property.kind, PropertyKind::Always | PropertyKind::Never))
        .count();
    let domain = model.fields.iter().try_fold(1_usize, |total, field| {
        count(&field.kind).and_then(|count| total.checked_mul(count))
    });
    let well_typed_states = match domain {
        Some(states) if states <= model.state_limit => states,
        _ => {
            let size = domain.map_or_else(|| "too many to count".to_string(), |n| n.to_string());
            return Err(Error::of(
                ErrorKind::Limit,
                None,
                format!(
                    "checking induction visits every well-typed state, and the field types allow {size}, more than the limit of {}; tighten the ranges or raise the limit with 'explore at most N states'",
                    model.state_limit
                ),
            ));
        }
    };
    let mut induction = Induction {
        properties,
        well_typed_states,
        satisfying_states: 0,
        counterexample: None,
    };
    if properties == 0 {
        return Ok(induction);
    }
    let fields: Vec<Vec<Vec<i64>>> = model.fields.iter().map(|f| values(&f.kind)).collect();
    let mut positions = vec![0_usize; fields.len()];
    let mut state: Vec<i64> = model.initial.to_vec();
    for (field, values) in model.fields.iter().zip(&fields) {
        place(&mut state, field.offset, values.first());
    }
    let mut locals = model.new_locals();
    let calls = index32(model.calls.len());
    let mut taken = 0_usize;
    loop {
        if satisfies(model, &state, &mut locals) == Some(true) {
            induction.satisfying_states += 1;
            if induction.counterexample.is_none() {
                for call in 0..calls {
                    let found = match model.step(call, &state, &mut locals) {
                        Ok(None) => continue,
                        Ok(Some(next)) => {
                            taken += 1;
                            if taken > MAX_TRANSITIONS {
                                return Err(Error::of(
                                    ErrorKind::Limit,
                                    None,
                                    format!(
                                        "checking induction takes more than {MAX_TRANSITIONS} calls"
                                    ),
                                ));
                            }
                            violation(model, &next, &mut locals).map(|broken| (Some(next), broken))
                        }
                        Err(fault) => Some((None, faulted(fault))),
                    };
                    if let Some((next, broken)) = found {
                        induction.counterexample = Some(InductionCounterexample {
                            state: model.decode(&state),
                            call: model.call_text(call),
                            next: next.map(|next| model.decode(&next)),
                            broken,
                        });
                        break;
                    }
                }
            }
        }
        if !advance(model, &fields, &mut positions, &mut state) {
            return Ok(induction);
        }
    }
}

/// Moves to the next state, the last field changing fastest; false after the last.
fn advance(
    model: &Model,
    fields: &[Vec<Vec<i64>>],
    positions: &mut [usize],
    state: &mut [i64],
) -> bool {
    for index in (0..fields.len()).rev() {
        let (Some(values), Some(position), Some(field)) = (
            fields.get(index),
            positions.get_mut(index),
            model.fields.get(index),
        ) else {
            return false;
        };
        *position += 1;
        if *position < values.len() {
            place(state, field.offset, values.get(*position));
            return true;
        }
        *position = 0;
        place(state, field.offset, values.first());
    }
    false
}

fn place(state: &mut [i64], offset: usize, value: Option<&Vec<i64>>) {
    if let Some(value) = value
        && let Some(slots) = state.get_mut(offset..offset + value.len())
    {
        slots.copy_from_slice(value);
    }
}

/// Whether every `always` and `never` property holds; `None` on a fault.
fn satisfies(model: &Model, state: &[i64], locals: &mut [i64]) -> Option<bool> {
    for property in &model.properties {
        let wanted = match property.kind {
            PropertyKind::Always => true,
            PropertyKind::Never => false,
            _ => continue,
        };
        if model.holds(&property.condition, state, locals).ok()? != wanted {
            return Some(false);
        }
    }
    Some(true)
}

/// The first property `state` breaks, or the fault evaluating one raises.
fn violation(model: &Model, state: &[i64], locals: &mut [i64]) -> Option<Broken> {
    for property in &model.properties {
        let wanted = match property.kind {
            PropertyKind::Always => true,
            PropertyKind::Never => false,
            _ => continue,
        };
        match model.holds(&property.condition, state, locals) {
            Ok(holds) if holds == wanted => {}
            Ok(_) => {
                return Some(Broken::Property {
                    text: property.text.clone(),
                    line: property.line,
                });
            }
            Err(fault) => return Some(faulted(fault)),
        }
    }
    None
}

fn faulted(fault: Fault) -> Broken {
    Broken::Fault {
        message: fault.message,
        line: fault.line,
    }
}

/// How many values a field of type `kind` can hold; `None` past `usize`.
fn count(kind: &FieldType) -> Option<usize> {
    match kind {
        FieldType::Scalar(domain) => Some(domain.size()),
        FieldType::Map { keys, value, .. } => {
            count(value)?.checked_pow(u32::try_from(keys.size()).ok()?)
        }
        FieldType::Record(members) => product(members.iter().map(|(_, kind)| count(kind))),
        FieldType::Optional(value) => count(value)?.checked_add(1),
        FieldType::Set { elements, .. } => {
            2_usize.checked_pow(u32::try_from(elements.size()).ok()?)
        }
        FieldType::List { capacity, element } => {
            let each = count(element)?;
            (0..=u32::try_from(*capacity).ok()?).try_fold(0_usize, |total, length| {
                total.checked_add(each.checked_pow(length)?)
            })
        }
        FieldType::Variant { cases, .. } => {
            cases.iter().try_fold(0_usize, |total, (_, members)| {
                total.checked_add(product(members.iter().map(|(_, kind)| count(kind)))?)
            })
        }
    }
}

fn product(mut counts: impl Iterator<Item = Option<usize>>) -> Option<usize> {
    counts.try_fold(1_usize, |total, count| total.checked_mul(count?))
}

/// Every value of type `kind`, each in its one encoding: the slots an absent
/// optional, the items past a list's length and the cases not taken would
/// hold keep their first values. Callers bound the count first.
fn values(kind: &FieldType) -> Vec<Vec<i64>> {
    match kind {
        FieldType::Scalar(domain) => domain.raws().into_iter().map(|raw| vec![raw]).collect(),
        FieldType::Map { keys, value, .. } => {
            let each = values(value);
            combine(&vec![each; keys.size()])
        }
        FieldType::Record(members) => combine(
            &members
                .iter()
                .map(|(_, kind)| values(kind))
                .collect::<Vec<_>>(),
        ),
        FieldType::Optional(value) => {
            let mut absent = vec![0];
            absent.extend(value.defaults());
            let mut out = vec![absent];
            out.extend(values(value).into_iter().map(|value| {
                let mut present = vec![1];
                present.extend(value);
                present
            }));
            out
        }
        FieldType::Set { elements, .. } => combine(&vec![vec![vec![0], vec![1]]; elements.size()]),
        FieldType::List { capacity, element } => {
            let each = values(element);
            let unused = element.defaults();
            let mut out = Vec::new();
            for length in 0..=*capacity {
                for items in combine(&vec![each.clone(); length]) {
                    let mut list = vec![i64::try_from(length).unwrap_or(i64::MAX)];
                    list.extend(items);
                    for _ in length..*capacity {
                        list.extend(&unused);
                    }
                    out.push(list);
                }
            }
            out
        }
        FieldType::Variant { tags, cases } => {
            let mut out = Vec::new();
            for (taken, (_, members)) in cases.iter().enumerate() {
                let taken_values = combine(
                    &members
                        .iter()
                        .map(|(_, kind)| values(kind))
                        .collect::<Vec<_>>(),
                );
                for chosen in taken_values {
                    let mut variant = vec![tags.raw_at(taken).unwrap_or(0)];
                    for (case, (_, members)) in cases.iter().enumerate() {
                        if case == taken {
                            variant.extend(&chosen);
                        } else {
                            variant.extend(members.iter().flat_map(|(_, kind)| kind.defaults()));
                        }
                    }
                    out.push(variant);
                }
            }
            out
        }
    }
}

/// Every concatenation of one value from each part, the first part changing slowest.
fn combine(parts: &[Vec<Vec<i64>>]) -> Vec<Vec<i64>> {
    parts.iter().fold(vec![Vec::new()], |prefixes, part| {
        prefixes
            .iter()
            .flat_map(|prefix| {
                part.iter().map(move |value| {
                    let mut joined = prefix.clone();
                    joined.extend(value);
                    joined
                })
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::{count, values};
    use crate::model::Model;
    use std::collections::BTreeSet;

    #[test]
    fn values_are_distinct_and_counted() {
        let spec = crate::parse(
            r#"system "Compound"
types
  color is one of red, blue
state
  lease is a record with (holder is color, left is an integer from 1 to 2) or nothing
  queue is a list of at most 2 color or nothing
  job is one of queued, running with (by is color or nothing), failed with (reasons is a set of color)
  marks is a map from color to a list of at most 1 color
initially
  lease is nothing
  queue is []
  job is queued
  marks of every color is []
"#,
        )
        .unwrap();
        let model = Model::compile(&spec).unwrap();
        for field in &model.fields {
            let all = values(&field.kind);
            assert_eq!(Some(all.len()), count(&field.kind), "{}", field.name);
            let width = field.kind.width();
            assert!(
                all.iter().all(|value| value.len() == width),
                "{}",
                field.name
            );
            let decoded: BTreeSet<_> = all
                .iter()
                .map(|value| model.decode_value(&field.kind, value))
                .collect();
            assert_eq!(decoded.len(), all.len(), "{}", field.name);
        }
    }
}
