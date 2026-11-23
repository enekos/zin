//! Observation adequacy: which states a conformance run can tell apart.
//!
//! A runner sees whether each call is accepted and, after every step, the
//! fields the adapter's `observe` returns. Two states with the same
//! observation are told apart only by a later call that one accepts and the
//! other rejects, or that leads them to different observations. This module
//! partitions the reachable states by that notion (Moore's refinement, seeded
//! with the observation) and keeps the refinement tree, so the shortest
//! sequence of calls separating two states can be read back from it.

use crate::check::Verification;
use crate::explore::Graph;
use crate::model::{FieldType, Model, at, index32};
use crate::plan::PlanStep;
use crate::{Error, ErrorKind, Result, State};
use std::collections::HashMap;

/// Signature entries hashed across all refinement rounds before giving up.
const MAX_REFINEMENT_WORK: usize = 100_000_000;
/// How many blind groups a report describes; the rest are only counted.
const LISTED_GROUPS: usize = 5;
const NONE: u32 = u32::MAX;

/// A set of states no shorter sequence of calls has told apart yet.
struct Node {
    parent: u32,
    depth: u32,
    /// The round in which this set split, which is the number of calls the
    /// shortest sequence separating two of its parts needs.
    split: u32,
    size: u32,
}

/// A call that starts separating two states: how many further calls their
/// targets need, the call, and the targets when both accept it.
type Opening = (u32, u32, Option<(u32, u32)>);

pub(crate) struct Classes {
    pub(crate) fields: Vec<String>,
    /// Per state, which observation it shows.
    observed: Vec<u32>,
    by_observation: Vec<Vec<u32>>,
    /// Per state, its set in the final partition.
    leaf: Vec<u32>,
    nodes: Vec<Node>,
    /// Two states that only the longest needed sequence separates.
    hardest: Option<(u32, u32)>,
}

/// The state-vector slots the named fields occupy, with the names in
/// declaration order and without repeats.
fn observed_slots(model: &Model, fields: &[String]) -> Result<(Vec<String>, Vec<usize>)> {
    for name in fields {
        if !model.fields.iter().any(|field| &field.name == name) {
            let known: Vec<&str> = model.fields.iter().map(|f| f.name.as_str()).collect();
            return Err(Error::usage(format!(
                "'{name}' is not a state field; the fields are {}",
                known.join(", ")
            )));
        }
    }
    let mut names = Vec::new();
    let mut slots = Vec::new();
    for field in model.fields.iter().filter(|f| fields.contains(&f.name)) {
        names.push(field.name.clone());
        match &field.kind {
            FieldType::Scalar(_) => slots.push(field.offset),
            FieldType::Map { keys, .. } => slots.extend(field.offset..field.offset + keys.size()),
        }
    }
    Ok((names, slots))
}

fn charge(work: &mut usize, amount: usize) -> Result<()> {
    *work = work.saturating_add(amount);
    if *work > MAX_REFINEMENT_WORK {
        return Err(Error::of(
            ErrorKind::Limit,
            None,
            "telling the states apart under this observation takes too long; observe more fields",
        ));
    }
    Ok(())
}

impl Classes {
    /// Partitions the states of a full (unreduced) graph by what a runner
    /// observing `fields` can ever tell apart.
    pub(crate) fn analyse(verification: &Verification, fields: &[String]) -> Result<Self> {
        let graph = &verification.graph;
        if graph.reduced() {
            return Err(Error::usage(
                "observation analysis needs concrete states; build it from verify(), which explores the full model",
            ));
        }
        let (fields, slots) = observed_slots(&verification.model, fields)?;
        let count = graph.state_count();
        let mut work = 0;

        let mut seen: HashMap<Vec<i64>, u32> = HashMap::new();
        let mut observed = Vec::with_capacity(count);
        let mut by_observation: Vec<Vec<u32>> = Vec::new();
        for state in 0..count {
            let values = graph.state(index32(state));
            let key: Vec<i64> = slots
                .iter()
                .map(|slot| values.get(*slot).copied().unwrap_or(0))
                .collect();
            charge(&mut work, 1 + key.len())?;
            let next = index32(seen.len());
            let id = *seen.entry(key).or_insert(next);
            if id == next {
                by_observation.push(Vec::new());
            }
            by_observation[at(id)].push(index32(state));
            observed.push(id);
        }

        let mut nodes = vec![Node {
            parent: NONE,
            depth: 0,
            split: NONE,
            size: index32(count),
        }];
        let mut leaf = vec![0; count];
        let mut hardest = None;
        if by_observation.len() > 1 {
            nodes[0].split = 0;
            for members in &by_observation {
                let node = index32(nodes.len());
                nodes.push(Node {
                    parent: 0,
                    depth: 1,
                    split: NONE,
                    size: index32(members.len()),
                });
                for state in members {
                    leaf[at(*state)] = node;
                }
            }
            hardest = Some((by_observation[0][0], by_observation[1][0]));
        }

        let mut classes = Self {
            fields,
            observed,
            by_observation,
            leaf,
            nodes,
            hardest,
        };
        let mut round = 1;
        while classes.refine(graph, round, &mut work)? {
            round += 1;
        }
        Ok(classes)
    }

    /// One round: splits every set whose states differ in which calls they
    /// accept or in the sets those calls lead to. Returns whether any split.
    fn refine(&mut self, graph: &Graph, round: u32, work: &mut usize) -> Result<bool> {
        let mut groups: HashMap<(u32, Vec<u32>), u32> = HashMap::new();
        // Per group, the set it came from and its lowest state.
        let mut origin: Vec<(u32, u32)> = Vec::new();
        let mut parts: HashMap<u32, u32> = HashMap::new();
        let mut assigned: Vec<(u32, u32)> = Vec::new();
        for state in 0..self.leaf.len() {
            let from = self.leaf[state];
            if self.nodes[at(from)].size < 2 {
                continue;
            }
            let state = index32(state);
            let out = graph.out(state);
            charge(work, 1 + out.len())?;
            let signature: Vec<u32> = out
                .iter()
                .flat_map(|edge| [edge.call, self.leaf[at(edge.to)]])
                .collect();
            let next = index32(origin.len());
            let group = *groups.entry((from, signature)).or_insert(next);
            if group == next {
                origin.push((from, state));
                *parts.entry(from).or_insert(0) += 1;
            }
            assigned.push((state, group));
        }

        let mut target = Vec::with_capacity(origin.len());
        let mut example: HashMap<u32, u32> = HashMap::new();
        let mut split = false;
        for (from, lowest) in &origin {
            if parts.get(from).copied().unwrap_or(0) < 2 {
                target.push(*from);
                continue;
            }
            target.push(index32(self.nodes.len()));
            self.nodes.push(Node {
                parent: *from,
                depth: self.nodes[at(*from)].depth + 1,
                split: NONE,
                size: 0,
            });
            self.nodes[at(*from)].split = round;
            match example.get(from) {
                Some(other) if !split => {
                    self.hardest = Some((*other, *lowest));
                    split = true;
                }
                Some(_) => {}
                None => {
                    example.insert(*from, *lowest);
                }
            }
        }
        for (state, group) in assigned {
            let node = target[at(group)];
            if node != self.leaf[at(state)] {
                self.leaf[at(state)] = node;
                self.nodes[at(node)].size += 1;
            }
        }
        Ok(split)
    }

    pub(crate) fn same_observation(&self, first: u32, second: u32) -> bool {
        self.observed.get(at(first)) == self.observed.get(at(second))
    }

    /// The states that show the same observation as `state` and that some
    /// sequence of calls tells apart from it.
    pub(crate) fn peers(&self, state: u32) -> Vec<u32> {
        let own = self.leaf[at(state)];
        self.by_observation[at(self.observed[at(state)])]
            .iter()
            .copied()
            .filter(|peer| self.leaf[at(*peer)] != own)
            .collect()
    }

    /// How many calls the shortest sequence separating the two states needs;
    /// `None` when no sequence separates them.
    fn split_round(&self, first: u32, second: u32) -> Option<u32> {
        let mut first = self.leaf[at(first)];
        let mut second = self.leaf[at(second)];
        while first != second {
            let (high, low) = (&self.nodes[at(first)], &self.nodes[at(second)]);
            if high.depth >= low.depth {
                first = high.parent;
            }
            if low.depth >= high.depth {
                second = low.parent;
            }
        }
        Some(self.nodes[at(first)].split).filter(|round| *round != NONE)
    }

    /// The shortest steps, as `first` answers them, that `second` answers
    /// differently or that end in a different observation. Empty when the
    /// observation already differs or nothing separates the two.
    pub(crate) fn sequence(&self, graph: &Graph, first: u32, second: u32) -> Vec<PlanStep> {
        let (mut first, mut second) = (first, second);
        let mut steps = Vec::new();
        while let Some(round) = self.split_round(first, second).filter(|round| *round > 0) {
            // The call whose targets need the fewest further calls; a call
            // only one of the two accepts needs none.
            let mut best: Option<Opening> = None;
            let (mut ours, mut theirs) = (graph.out(first).iter(), graph.out(second).iter());
            let (mut a, mut b) = (ours.next(), theirs.next());
            while a.is_some() || b.is_some() {
                let candidate = match (a, b) {
                    (Some(x), Some(y)) if x.call == y.call => {
                        a = ours.next();
                        b = theirs.next();
                        self.split_round(x.to, y.to)
                            .filter(|rest| *rest < round)
                            .map(|rest| (rest, x.call, Some((x.to, y.to))))
                    }
                    (Some(x), Some(y)) if x.call < y.call => {
                        a = ours.next();
                        Some((0, x.call, None))
                    }
                    (Some(x), None) => {
                        a = ours.next();
                        Some((0, x.call, None))
                    }
                    (_, Some(y)) => {
                        b = theirs.next();
                        Some((0, y.call, None))
                    }
                    (None, None) => None,
                };
                if let Some(candidate) = candidate
                    && best.is_none_or(|best| candidate.0 < best.0)
                {
                    best = Some(candidate);
                }
            }
            let Some((_, call, targets)) = best else {
                break;
            };
            steps.push(PlanStep {
                call,
                to: graph.target(first, call),
            });
            let Some(targets) = targets else { break };
            (first, second) = targets;
        }
        steps
    }
}

/// States that show the same observation and that no sequence of calls
/// tells apart: a runner can never notice the implementation being in one
/// instead of another.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct BlindGroup {
    pub states: usize,
    /// Two members of the group.
    pub examples: Vec<State>,
    /// The fields whose values differ within the group.
    pub differ: Vec<String>,
}

/// Two states with the same observation that need the longest sequence of
/// calls to tell apart.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Hardest {
    pub first: State,
    pub second: State,
    /// The calls from `first`, each prefixed with `reject ` when `first` refuses it.
    pub calls: Vec<String>,
}

/// What observing a set of fields lets a conformance run tell apart.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Adequacy {
    /// The observed fields, in declaration order.
    pub fields: Vec<String>,
    pub states: usize,
    /// Distinct values of the observed fields over the reachable states.
    pub observations: usize,
    /// The most calls two distinguishable states need to be told apart.
    pub depth: usize,
    pub hardest: Option<Hardest>,
    pub blind_groups: usize,
    pub blind_states: usize,
    /// The first few blind groups; `blind_groups` counts all of them.
    pub blind: Vec<BlindGroup>,
}

impl Adequacy {
    /// Whether every pair of states can be told apart by some run.
    #[must_use]
    pub fn adequate(&self) -> bool {
        self.blind_groups == 0
    }
}

/// Reports which states a runner observing `fields` (any subset of the state
/// fields, possibly none) can tell apart. Needs the full graph from
/// [`crate::verify`].
pub fn adequacy(verification: &Verification, fields: &[String]) -> Result<Adequacy> {
    let classes = Classes::analyse(verification, fields)?;
    Ok(classes.report(verification))
}

impl Classes {
    pub(crate) fn blind_states(&self) -> usize {
        self.leaf
            .iter()
            .filter(|leaf| self.nodes[at(**leaf)].size > 1)
            .count()
    }

    pub(crate) fn report(&self, verification: &Verification) -> Adequacy {
        let model = &verification.model;
        let graph = &verification.graph;
        let mut index: HashMap<u32, usize> = HashMap::new();
        let mut groups: Vec<Vec<u32>> = Vec::new();
        for (state, leaf) in self.leaf.iter().enumerate() {
            if self.nodes[at(*leaf)].size < 2 {
                continue;
            }
            let next = groups.len();
            let group = *index.entry(*leaf).or_insert(next);
            if group == next {
                groups.push(Vec::new());
            }
            // Only listed groups keep all their members.
            if group < LISTED_GROUPS || groups[group].is_empty() {
                groups[group].push(index32(state));
            }
        }
        let blind = groups
            .iter()
            .take(LISTED_GROUPS)
            .map(|members| {
                let lowest = graph.state(members[0]);
                let differ = model
                    .fields
                    .iter()
                    .filter(|field| {
                        let width = match &field.kind {
                            FieldType::Scalar(_) => 1,
                            FieldType::Map { keys, .. } => keys.size(),
                        };
                        let slots = field.offset..field.offset + width;
                        members.iter().any(|state| {
                            graph.state(*state).get(slots.clone()) != lowest.get(slots.clone())
                        })
                    })
                    .map(|field| field.name.clone())
                    .collect();
                BlindGroup {
                    states: members.len(),
                    examples: members
                        .iter()
                        .take(2)
                        .map(|state| verification.state(*state))
                        .collect(),
                    differ,
                }
            })
            .collect();
        let depth = self
            .nodes
            .iter()
            .map(|node| node.split)
            .filter(|round| *round != NONE)
            .max()
            .unwrap_or(0);
        let hardest = self
            .hardest
            .filter(|_| depth > 0)
            .map(|(first, second)| Hardest {
                first: verification.state(first),
                second: verification.state(second),
                calls: self
                    .sequence(graph, first, second)
                    .iter()
                    .map(|step| {
                        let text = model.call_text(step.call);
                        match step.to {
                            Some(_) => text,
                            None => format!("reject {text}"),
                        }
                    })
                    .collect(),
            });
        Adequacy {
            fields: self.fields.clone(),
            states: graph.state_count(),
            observations: self.by_observation.len(),
            depth: at(depth),
            hardest,
            blind_groups: groups.len(),
            blind_states: self.blind_states(),
            blind,
        }
    }
}
