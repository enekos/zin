//! Adjacent-call coverage: every feasible (state, first call, second call)
//! obligation, chained into tours instead of replayed from a fresh setup.
use super::{Budget, PlanStep, route_to_pending};
use crate::explore::Graph;
use crate::model::at;
use crate::{Error, ErrorKind, Result};
use std::collections::{BTreeSet, HashSet};

/// The state before the first call, then both call IDs. A refusal leaves the state unchanged.
pub(super) type Pair = (u32, u32, u32);

pub(super) struct Pairs {
    by_state: Vec<BTreeSet<(u32, u32)>>,
    total: usize,
    pub(super) covered: HashSet<Pair>,
}

impl Pairs {
    /// Selects `target` obligations deterministically: repetitions first, then
    /// each rotation of the second call, interleaving states.
    pub(super) fn select(graph: &Graph, calls: u32, target: usize) -> Self {
        let mut by_state = vec![BTreeSet::new(); graph.state_count()];
        let mut total = 0;
        'select: for offset in 0..calls {
            for first in 0..calls {
                let second = (first + offset) % calls;
                for state in by_state.iter_mut() {
                    if total == target {
                        break 'select;
                    }
                    state.insert((first, second));
                    total += 1;
                }
            }
        }
        Self {
            by_state,
            total,
            covered: HashSet::new(),
        }
    }

    pub(super) fn record(&mut self, steps: &[PlanStep]) {
        let mut state = 0;
        for window in steps.windows(2) {
            self.cover((state, window[0].call, window[1].call));
            state = window[0].to.unwrap_or(state);
        }
    }

    fn cover(&mut self, (state, first, second): Pair) {
        self.covered.insert((state, first, second));
        if self.by_state[at(state)].remove(&(first, second)) {
            self.total -= 1;
        }
    }

    fn waiting(&self, state: u32) -> bool {
        !self.by_state[at(state)].is_empty()
    }

    /// A second call for `(from, first)` that leaves the walk where another pair can start.
    fn completion(&self, from: u32, first: u32, here: u32) -> Option<u32> {
        let open = &self.by_state[at(from)];
        let mut seconds = open
            .range((first, 0)..=(first, u32::MAX))
            .map(|(_, second)| *second);
        let fallback = seconds.clone().next()?;
        let starts = |call: u32| {
            self.by_state[at(here)]
                .range((call, 0)..=(call, u32::MAX))
                .next()
                .is_some()
        };
        Some(seconds.find(|call| starts(*call)).unwrap_or(fallback))
    }
}

struct Walk<'a> {
    graph: &'a Graph,
    steps: Vec<PlanStep>,
    state: u32,
    last: Option<(u32, u32)>,
}

impl Walk<'_> {
    fn call(&mut self, pairs: &mut Pairs, budget: &mut Budget, call: u32) -> Result<()> {
        budget.charge(1)?;
        let to = self.graph.target(self.state, call);
        if let Some((from, first)) = self.last {
            pairs.cover((from, first, call));
        }
        self.steps.push(PlanStep { call, to });
        self.last = Some((self.state, call));
        self.state = to.unwrap_or(self.state);
        Ok(())
    }
}

pub(super) fn pair_tours(
    graph: &Graph,
    pairs: &mut Pairs,
    max_steps: usize,
    depths: &[usize],
    budget: &mut Budget,
) -> Result<Vec<Vec<PlanStep>>> {
    // Each step completes at most one pair, so this bound is exact for a single chain.
    if pairs.total > budget.remaining {
        return Err(Error::of(
            ErrorKind::Limit,
            None,
            format!(
                "covering {} adjacent-call pairs needs more than the {} remaining replay steps; use --pair-limit to sample pairs",
                pairs.total, budget.remaining
            ),
        ));
    }
    let mut tours = Vec::new();
    while pairs.total > 0 {
        let initial_pending = pairs.total;
        let mut walk = Walk {
            graph,
            steps: Vec::new(),
            state: 0,
            last: None,
        };
        loop {
            let completion = walk
                .last
                .and_then(|(from, first)| pairs.completion(from, first, walk.state));
            let start = pairs.by_state[at(walk.state)]
                .iter()
                .next()
                .map(|(first, _)| *first);
            if let Some(call) = completion.or(start) {
                walk.call(pairs, budget, call)?;
            } else {
                let route = route_to_pending(graph, |state| pairs.waiting(state), walk.state);
                let Some((_, last)) = route.last() else { break };
                let destination = graph.edges[*last].to;
                if pairs.total < initial_pending
                    && walk.steps.len() + route.len()
                        >= max_steps.saturating_add(depths[at(destination)])
                {
                    break;
                }
                for (_, index) in route {
                    walk.call(pairs, budget, graph.edges[index].call)?;
                }
            }
            if pairs.total == 0
                || (pairs.total < initial_pending
                    && walk.last.is_none_or(|(from, first)| {
                        pairs.completion(from, first, walk.state).is_none()
                    })
                    && walk.steps.len().saturating_sub(depths[at(walk.state)]) >= max_steps)
            {
                break;
            }
        }
        if initial_pending == pairs.total {
            return Err(Error::new(
                "planner could not reach its remaining pair obligations",
            ));
        }
        tours.push(walk.steps);
    }
    Ok(tours)
}

/// Number of (state, first, second) obligations, or a limit error when it
/// cannot be represented in a portable plan.
pub(super) fn eligible(graph: &Graph, calls: u32) -> Result<u64> {
    let eligible = u64::try_from(graph.state_count())
        .ok()
        .and_then(|states| states.checked_mul(u64::from(calls).checked_pow(2)?))
        .filter(|n| *n <= super::MAX_SAFE_INTEGER.unsigned_abs())
        .ok_or_else(|| {
            Error::of(
                ErrorKind::Limit,
                None,
                "pair count exceeds the portable plan integer range",
            )
        })?;
    Ok(eligible)
}
