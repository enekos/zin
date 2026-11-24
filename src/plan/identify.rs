//! State identification: confirm that each transition the plan takes lands
//! in the state the specification names, not merely in one that shows the
//! same observation.
//!
//! For a transition into `s` and every state `p` that looks like `s` but
//! behaves differently, some test must continue after the transition with
//! steps that `p` would answer differently. The cover's own tests often do;
//! where they do not, a test is added: the shortest setup, the transition,
//! then the shortest sequence separating `s` from `p`.
use super::{Budget, PlanStep, PlanTest, TestKind, edge_index};
use crate::explore::Graph;
use crate::observe::Classes;
use crate::{Error, ErrorKind, Result};
use std::collections::BTreeMap;

/// Look-alike states still to rule out, summed over all transitions.
const MAX_PENDING_PEERS: usize = 4_000_000;
/// Steps simulated on look-alike states before giving up.
const MAX_WORK: usize = 200_000_000;
/// How many look-alikes' separating sequences are tried to find the one that
/// rules out the most.
const CANDIDATES: usize = 8;

struct Obligation {
    from: u32,
    call: u32,
    to: u32,
    /// States the implementation could be in instead of `to` without any
    /// test noticing so far.
    peers: Vec<u32>,
}

struct Identifier<'a> {
    graph: &'a Graph,
    classes: &'a Classes,
    /// Look-alikes of the initial state not ruled out yet.
    start: Vec<u32>,
    edges: BTreeMap<usize, Obligation>,
    work: usize,
}

fn limit(what: &str) -> Error {
    Error::of(
        ErrorKind::Limit,
        None,
        format!("identifying states under this observation {what}; observe more fields"),
    )
}

/// Removes from `peers` every state that answers `steps` differently from
/// the state the specification expects before them.
fn separate(
    graph: &Graph,
    classes: &Classes,
    steps: &[PlanStep],
    peers: &mut Vec<u32>,
    work: &mut usize,
) -> Result<()> {
    let mut failure = None;
    peers.retain(|peer| {
        let mut actual = *peer;
        for step in steps {
            *work += 1;
            match (step.to, graph.target(actual, step.call)) {
                (None, None) => {}
                (Some(to), Some(next)) => {
                    if !classes.same_observation(to, next) {
                        return false;
                    }
                    // From the same state on, nothing can differ.
                    if to == next {
                        break;
                    }
                    actual = next;
                }
                _ => return false,
            }
        }
        if *work > MAX_WORK {
            failure = Some(limit("takes too long"));
        }
        true
    });
    failure.map_or(Ok(()), Err)
}

impl Identifier<'_> {
    /// Lets `steps`, a whole test, rule out look-alikes of every state it
    /// passes through. With `adopt`, transitions seen for the first time
    /// become obligations.
    fn scan(&mut self, from: u32, steps: &[PlanStep], adopt: bool) -> Result<()> {
        let (graph, classes) = (self.graph, self.classes);
        if from == 0 {
            separate(graph, classes, steps, &mut self.start, &mut self.work)?;
        }
        let mut state = from;
        for (index, step) in steps.iter().enumerate() {
            let Some(to) = step.to else { continue };
            if let Some(edge) = edge_index(graph, state, step.call) {
                if adopt {
                    self.edges.entry(edge).or_insert_with(|| Obligation {
                        from: state,
                        call: step.call,
                        to,
                        peers: classes.peers(to),
                    });
                }
                if let Some(obligation) = self.edges.get_mut(&edge) {
                    let rest = steps.get(index + 1..).unwrap_or(&[]);
                    separate(graph, classes, rest, &mut obligation.peers, &mut self.work)?;
                    obligation.peers.shrink_to_fit();
                }
            }
            state = to;
        }
        Ok(())
    }

    fn pending(&self) -> usize {
        self.edges.values().map(|edge| edge.peers.len()).sum()
    }

    /// The steps from `state` that rule out the most of `peers`, among the
    /// shortest sequences separating it from each of the first few.
    fn best_sequence(&mut self, state: u32, peers: &[u32]) -> Result<Vec<PlanStep>> {
        let mut best: Option<(usize, Vec<PlanStep>)> = None;
        for peer in peers.iter().take(CANDIDATES) {
            let steps = self.classes.sequence(self.graph, state, *peer);
            if best.as_ref().is_some_and(|(_, chosen)| *chosen == steps) {
                continue;
            }
            let mut left = peers.to_vec();
            separate(self.graph, self.classes, &steps, &mut left, &mut self.work)?;
            if best.as_ref().is_none_or(|(fewest, _)| left.len() < *fewest) {
                best = Some((left.len(), steps));
            }
        }
        Ok(best.map(|(_, steps)| steps).unwrap_or_default())
    }
}

pub(super) struct Identified {
    /// Distinct transitions the cover's tests take.
    pub(super) transitions: usize,
    /// Of those, the ones whose target the cover's tests did not confirm.
    pub(super) unconfirmed: usize,
}

/// Appends the tests that rule out every look-alike of the initial state
/// and of the target of every transition `tests` take.
pub(super) fn identify(
    graph: &Graph,
    classes: &Classes,
    tests: &mut Vec<PlanTest>,
    budget: &mut Budget,
) -> Result<Identified> {
    let mut identifier = Identifier {
        graph,
        classes,
        start: classes.peers(0),
        edges: BTreeMap::new(),
        work: 0,
    };
    for test in tests.iter() {
        identifier.scan(test.from, &test.steps, true)?;
        if identifier.pending() > MAX_PENDING_PEERS {
            return Err(limit("leaves too many look-alike states to rule out"));
        }
    }
    let identified = Identified {
        transitions: identifier.edges.len(),
        unconfirmed: identifier
            .edges
            .values()
            .filter(|edge| !edge.peers.is_empty())
            .count(),
    };

    // A restored start has no incoming transition. Check its look-alikes
    // against the continuations that actually start there.
    let mut starts = BTreeMap::new();
    for test in tests.iter().filter(|test| test.from != 0) {
        let peers = starts
            .entry(test.from)
            .or_insert_with(|| classes.peers(test.from));
        separate(graph, classes, &test.steps, peers, &mut identifier.work)?;
    }
    let mut added = 0;
    let mut emit = |identifier: &mut Identifier, from: u32, steps: Vec<PlanStep>| -> Result<()> {
        budget.charge(steps.len())?;
        identifier.scan(from, &steps, false)?;
        added += 1;
        tests.push(PlanTest {
            from,
            name: format!("identify {added}"),
            kind: TestKind::Identify,
            steps,
        });
        Ok(())
    };
    let stuck = || Error::new("planner could not tell two distinguishable states apart");
    while !identifier.start.is_empty() {
        let before = identifier.start.len();
        let peers = std::mem::take(&mut identifier.start);
        let steps = identifier.best_sequence(0, &peers)?;
        identifier.start = peers;
        emit(&mut identifier, 0, steps)?;
        if identifier.start.len() == before {
            return Err(stuck());
        }
    }
    let edges: Vec<usize> = identifier.edges.keys().copied().collect();
    for edge in edges {
        while let Some(obligation) = identifier.edges.get_mut(&edge) {
            if obligation.peers.is_empty() {
                break;
            }
            let (from, call, to) = (obligation.from, obligation.call, obligation.to);
            let peers = std::mem::take(&mut obligation.peers);
            let sequence = identifier.best_sequence(to, &peers)?;
            let before = peers.len();
            if let Some(obligation) = identifier.edges.get_mut(&edge) {
                obligation.peers = peers;
            }
            let mut steps: Vec<PlanStep> = graph
                .path_to(from)
                .into_iter()
                .map(|edge| PlanStep {
                    call: edge.call,
                    to: Some(edge.to),
                })
                .collect();
            steps.push(PlanStep { call, to: Some(to) });
            steps.extend(sequence);
            emit(&mut identifier, 0, steps)?;
            let unmoved = identifier
                .edges
                .get(&edge)
                .is_some_and(|obligation| obligation.peers.len() == before);
            if unmoved {
                return Err(stuck());
            }
        }
    }
    for (from, mut peers) in starts {
        while !peers.is_empty() {
            let before = peers.len();
            let steps = identifier.best_sequence(from, &peers)?;
            separate(graph, classes, &steps, &mut peers, &mut identifier.work)?;
            if peers.len() == before {
                return Err(stuck());
            }
            emit(&mut identifier, from, steps)?;
        }
    }
    Ok(identified)
}
