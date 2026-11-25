//! Symmetry reduction for `interchangeable` types.
//!
//! When a specification never tells the values of a type apart, relabeling
//! them maps reachable states to reachable states and preserves every
//! property. Exploration then keeps one representative per class of
//! relabeled states: the lexicographically smallest relabeling of each state.
//! Counterexamples found on representatives are replayed on the real state
//! space, so they read as ordinary runs.

use crate::explore::Edge;
use crate::model::{Call, Domain, FieldType, Model, at};
use crate::{Error, Result, Trace, TraceStep};
use std::cmp::Ordering;

/// Cap on the number of relabelings (8 interchangeable values: 8! = 40,320).
pub(crate) const MAX_RELABELINGS: usize = 40_320;
/// Cap on the precomputed tables, relabelings times state slots.
const MAX_TABLE_CELLS: usize = 8_000_000;

#[derive(Debug, Clone)]
pub(crate) struct Symmetry {
    /// Every relabeling, the identity first.
    relabelings: Vec<Relabeling>,
    /// Whether each slot holds a value that relabeling changes.
    relabeled: Vec<bool>,
}

#[derive(Debug, Clone)]
struct Relabeling {
    /// Raw enum value to raw enum value; values outside the table stay put.
    values: Vec<i64>,
    /// The relabeled state's slot `j` comes from the original's `source[j]`.
    source: Vec<usize>,
}

impl Relabeling {
    fn value(&self, raw: i64) -> i64 {
        usize::try_from(raw)
            .ok()
            .and_then(|index| self.values.get(index))
            .copied()
            .unwrap_or(raw)
    }
}

/// Whether relabeling `groups` changes a value of `domain`.
fn moved(domain: &Domain, groups: &[Vec<i64>]) -> bool {
    match domain {
        Domain::Enum(ids) => ids
            .iter()
            .any(|id| groups.iter().any(|group| group.contains(&i64::from(*id)))),
        Domain::Integer { .. } | Domain::Boolean => false,
    }
}

/// Where each slot of a relabeled value of type `kind` comes from: slot `j`
/// of the image is slot `sources[j]` of the original, both counted from the
/// value's first slot. Entries of a map keyed by a relabeled type move.
fn sources(kind: &FieldType, relabel: &impl Fn(i64) -> i64, groups: &[Vec<i64>]) -> Vec<usize> {
    match kind {
        FieldType::Scalar(_) => vec![0],
        FieldType::Map { keys, value, .. } => {
            let inner = sources(value, relabel, groups);
            let width = inner.len();
            let moves = moved(keys, groups);
            let mut out = vec![0; keys.size().saturating_mul(width)];
            for (position, key) in keys.raws().into_iter().enumerate() {
                let target = if moves {
                    keys.position(relabel(key)).unwrap_or(position)
                } else {
                    position
                };
                for (offset, from) in inner.iter().enumerate() {
                    if let Some(cell) = out.get_mut(target * width + offset) {
                        *cell = position * width + from;
                    }
                }
            }
            out
        }
    }
}

fn relabeling(model: &Model, values: Vec<i64>, groups: &[Vec<i64>]) -> Relabeling {
    let mut relabeling = Relabeling {
        values,
        source: (0..model.slots.len()).collect(),
    };
    for field in &model.fields {
        let relabel = |raw: i64| relabeling.value(raw);
        let moved = sources(&field.kind, &relabel, groups);
        for (offset, from) in moved.into_iter().enumerate() {
            if let Some(cell) = relabeling.source.get_mut(field.offset + offset) {
                *cell = field.offset + from;
            }
        }
    }
    relabeling
}

/// Adjacent transpositions generate the permutation group. Checking these
/// generators validates the initial state without constructing a factorial table.
pub(crate) fn validate(model: &Model) -> Result<()> {
    let mut count = 1usize;
    for group in &model.interchangeable {
        for n in 2..=group.len() {
            count = count.saturating_mul(n);
            if count > MAX_RELABELINGS {
                return Err(Error::new(format!(
                    "interchangeable types allow at most {MAX_RELABELINGS} relabelings in total (8 values); use fewer values"
                )));
            }
        }
    }
    let symmetry = Symmetry {
        relabelings: Vec::new(),
        relabeled: relabeled(model, &model.interchangeable),
    };
    for group in &model.interchangeable {
        for pair in group.windows(2) {
            let mut values: Vec<i64> = (0..symbol_count(model)).collect();
            let (first, second) = (at_raw(pair[0]), at_raw(pair[1]));
            if first < values.len() && second < values.len() {
                values.swap(first, second);
            }
            let swap = relabeling(model, values, &model.interchangeable);
            if *symmetry.apply(&swap, &model.initial) != *model.initial {
                return Err(Error::new(
                    "the initial state treats interchangeable values differently",
                ));
            }
        }
    }
    Ok(())
}

fn relabeled(model: &Model, groups: &[Vec<i64>]) -> Vec<bool> {
    model
        .slots
        .iter()
        .map(|slot| moved(&slot.domain, groups))
        .collect()
}

impl Symmetry {
    pub(crate) fn relabeling_count(&self) -> usize {
        self.relabelings.len()
    }
    /// Relabelings for `groups`, each a list of interchangeable raw values.
    pub(crate) fn new(model: &Model, groups: &[Vec<i64>]) -> Result<Self> {
        let n = groups.iter().flat_map(|g| 2..=g.len()).product::<usize>();
        let width = model.slots.len().saturating_add(model.symbols.len()).max(1);
        if n.saturating_mul(width) > MAX_TABLE_CELLS {
            return Err(Error::new(format!(
                "its relabeling tables would need more than {MAX_TABLE_CELLS} cells"
            )));
        }
        let mut maps: Vec<Vec<i64>> = vec![(0..symbol_count(model)).collect()];
        for group in groups {
            let orders = permutations(group.len());
            let mut extended = Vec::with_capacity(maps.len() * orders.len());
            for map in &maps {
                for order in &orders {
                    let mut map = map.clone();
                    for (from, to) in group.iter().zip(order) {
                        if let (Some(cell), Some(to)) = (map.get_mut(at_raw(*from)), group.get(*to))
                        {
                            *cell = *to;
                        }
                    }
                    extended.push(map);
                }
            }
            maps = extended;
        }
        let relabelings = maps
            .into_iter()
            .map(|values| relabeling(model, values, groups))
            .collect();
        Ok(Self {
            relabelings,
            relabeled: relabeled(model, groups),
        })
    }

    fn image_at(&self, relabeling: &Relabeling, state: &[i64], slot: usize) -> i64 {
        let raw = relabeling
            .source
            .get(slot)
            .and_then(|source| state.get(*source))
            .copied()
            .unwrap_or(0);
        if self.relabeled.get(slot).copied().unwrap_or(false) {
            relabeling.value(raw)
        } else {
            raw
        }
    }

    fn apply(&self, relabeling: &Relabeling, state: &[i64]) -> Box<[i64]> {
        (0..state.len())
            .map(|slot| self.image_at(relabeling, state, slot))
            .collect()
    }

    /// The representative of `state`: its smallest relabeling.
    pub(crate) fn canonical(&self, state: Box<[i64]>) -> Box<[i64]> {
        let mut best = state.clone();
        for relabeling in self.relabelings.iter().skip(1) {
            let mut ordering = Ordering::Equal;
            for (slot, current) in best.iter().enumerate() {
                ordering = self.image_at(relabeling, &state, slot).cmp(current);
                if ordering != Ordering::Equal {
                    break;
                }
            }
            if ordering == Ordering::Less {
                best = self.apply(relabeling, &state);
            }
        }
        best
    }

    fn relabeling_onto(&self, from: &[i64], onto: &[i64]) -> Option<&Relabeling> {
        self.relabelings.iter().find(|relabeling| {
            (0..from.len())
                .all(|slot| Some(&self.image_at(relabeling, from, slot)) == onto.get(slot))
        })
    }

    fn relabel_call(&self, model: &Model, relabeling: &Relabeling, call: u32) -> Option<u32> {
        let call = model.calls.get(at(call))?;
        let action = model.actions.get(call.action)?;
        let args = action
            .params
            .iter()
            .zip(&call.args)
            .map(|(param, raw)| match param.domain {
                Domain::Enum(_) => relabeling.value(*raw),
                Domain::Integer { .. } | Domain::Boolean => *raw,
            })
            .collect();
        model
            .call_ids
            .get(&Call {
                action: call.action,
                args,
            })
            .copied()
    }

    /// The prefix and the failing call must use the same concrete worker names.
    pub(crate) fn lift_fault(
        &self,
        model: &Model,
        states: &[Box<[i64]>],
        path: &[Edge],
        call: u32,
    ) -> Option<Error> {
        let (trace, concrete) = self.lift(model, states, path, None)?;
        let end = path.last().map_or(0, |edge| edge.to);
        let renaming = self.relabeling_onto(states.get(at(end))?, &concrete)?;
        let call = self.relabel_call(model, renaming, call)?;
        let fault = model.step(call, &concrete, &mut model.new_locals()).err()?;
        Some(
            Error::of(
                crate::ErrorKind::Model,
                Some(fault.line),
                format!("{}: {}", model.call_text(call), fault.message),
            )
            .with_trace(trace),
        )
    }

    /// Replays a path through representatives on the real state space.
    ///
    /// Each step finds a relabeling that takes the representative to the
    /// concrete state reached so far and takes the relabeled call. A loop
    /// may come back to a relabeling of where it started rather than the
    /// same state; it is then repeated until a concrete state recurs, which
    /// happens within as many rounds as there are relabelings.
    pub(crate) fn lift(
        &self,
        model: &Model,
        states: &[Box<[i64]>],
        path: &[Edge],
        loop_from: Option<usize>,
    ) -> Option<(Trace, Box<[i64]>)> {
        let representative = |state: u32| states.get(at(state)).map(|state| &**state);
        let mut locals = model.new_locals();
        let mut concrete: Box<[i64]> = representative(0)?.into();
        let mut from = 0;
        let mut trace = Trace {
            initial: model.decode(&concrete),
            steps: Vec::with_capacity(path.len()),
            loop_from: None,
        };
        let mut walk = |edges: &[Edge],
                        concrete: &mut Box<[i64]>,
                        from: &mut u32,
                        steps: &mut Vec<TraceStep>|
         -> Option<()> {
            for edge in edges {
                let relabeling = self.relabeling_onto(representative(*from)?, concrete)?;
                let call = self.relabel_call(model, relabeling, edge.call)?;
                *concrete = model.step(call, concrete, &mut locals).ok()??;
                steps.push(TraceStep {
                    call: model.call_text(call),
                    state: model.decode(concrete),
                });
                *from = edge.to;
            }
            Some(())
        };
        let (prefix, cycle) = path.split_at(loop_from.unwrap_or(path.len()).min(path.len()));
        walk(prefix, &mut concrete, &mut from, &mut trace.steps)?;
        if loop_from.is_none() {
            return Some((trace, concrete));
        }
        let mut starts: Vec<(Box<[i64]>, usize)> = Vec::new();
        for _ in 0..=self.relabelings.len() {
            if let Some((_, step)) = starts.iter().find(|(state, _)| *state == concrete) {
                trace.loop_from = Some(*step);
                return Some((trace, concrete));
            }
            starts.push((concrete.clone(), trace.steps.len()));
            walk(cycle, &mut concrete, &mut from, &mut trace.steps)?;
        }
        None
    }
}

fn symbol_count(model: &Model) -> i64 {
    i64::try_from(model.symbols.len()).unwrap_or(i64::MAX)
}

fn at_raw(raw: i64) -> usize {
    usize::try_from(raw).unwrap_or(usize::MAX)
}

/// Every ordering of `0..count`, in lexicographic order.
fn permutations(count: usize) -> Vec<Vec<usize>> {
    let mut current: Vec<usize> = (0..count).collect();
    let mut all = vec![current.clone()];
    loop {
        let Some(pivot) = (1..current.len())
            .rev()
            .find(|index| current.get(index - 1) < current.get(*index))
            .map(|index| index - 1)
        else {
            return all;
        };
        let Some(pivot_value) = current.get(pivot).copied() else {
            return all;
        };
        let Some(swap) = (pivot + 1..current.len()).rev().find(|index| {
            current
                .get(*index)
                .is_some_and(|value| *value > pivot_value)
        }) else {
            return all;
        };
        current.swap(pivot, swap);
        if let Some(tail) = current.get_mut(pivot + 1..) {
            tail.reverse();
        }
        all.push(current.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::permutations;

    #[test]
    fn permutations_are_complete_and_distinct() {
        let all = permutations(4);
        assert_eq!(all.len(), 24);
        let mut sorted = all.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, all);
        assert_eq!(permutations(0), vec![Vec::<usize>::new()]);
    }
}
