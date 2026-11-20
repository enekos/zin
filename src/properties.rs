use crate::ast::{Fairness, PropertyKind};
use crate::check::PropertyResult;
use crate::explore::{Edge, Graph};
use crate::model::{CompiledCondition, CompiledProperty, Model, at, index32};
use crate::{Error, ErrorKind, Result};
use std::collections::{HashMap, VecDeque};

const UNVISITED: u32 = u32::MAX;

pub(crate) struct Checked {
    pub results: Vec<PropertyResult>,
    pub notes: Vec<String>,
}

pub(crate) fn evaluate(
    model: &Model,
    graph: &Graph,
    condition: &CompiledCondition,
) -> Result<Vec<bool>> {
    let mut locals = model.new_locals();
    let mut result = Vec::with_capacity(graph.states.len());
    for (index, state) in graph.states.iter().enumerate() {
        let holds = model
            .holds(condition, state, &mut locals)
            .map_err(|fault| {
                Error::of(ErrorKind::Model, Some(fault.line), fault.message)
                    .with_trace(graph.trace_to(model, index32(index)))
            })?;
        result.push(holds);
    }
    Ok(result)
}

pub(crate) fn check_properties(model: &Model, graph: &Graph) -> Result<Checked> {
    let mut results = Vec::new();
    let mut notes = Vec::new();
    let mut predecessors: Option<Vec<Vec<u32>>> = None;
    for property in &model.properties {
        let mut witness = None;
        match property.kind {
            PropertyKind::Always | PropertyKind::Never => {}
            PropertyKind::Sometimes => {
                let holds = evaluate(model, graph, &property.condition)?;
                let first = holds.iter().position(|holds| *holds).ok_or_else(|| {
                    Error::of(
                        ErrorKind::Violation,
                        Some(property.line),
                        format!("'{}' fails: no reachable state satisfies it", property.text),
                    )
                })?;
                witness = Some(index32(first));
            }
            PropertyKind::Eventually | PropertyKind::LeadsTo => {
                let predecessors = predecessors.get_or_insert_with(|| graph.predecessors());
                if let Some(note) = liveness(model, graph, property, predecessors)? {
                    notes.push(note);
                }
            }
            PropertyKind::AlwaysPossible => {
                let predecessors = predecessors.get_or_insert_with(|| graph.predecessors());
                let goal = evaluate(model, graph, &property.condition)?;
                let can_reach = backward(&goal, predecessors, |_| true);
                if let Some(stuck) = can_reach.iter().position(|reachable| !reachable) {
                    return Err(Error::of(
                        ErrorKind::Violation,
                        Some(property.line),
                        format!(
                            "'{}' fails: after this execution it can never happen again",
                            property.text
                        ),
                    )
                    .with_trace(graph.trace_to(model, index32(stuck))));
                }
            }
        }
        results.push(PropertyResult {
            text: property.text.clone(),
            kind: property.kind,
            line: property.line,
            witness,
        });
    }
    Ok(Checked { results, notes })
}

fn backward(
    seeds: &[bool],
    predecessors: &[Vec<u32>],
    allowed: impl Fn(usize) -> bool,
) -> Vec<bool> {
    let mut marked = seeds.to_vec();
    let mut queue: VecDeque<usize> = seeds
        .iter()
        .enumerate()
        .filter(|(_, seed)| **seed)
        .map(|(index, _)| index)
        .collect();
    while let Some(state) = queue.pop_front() {
        for previous in predecessors.get(state).map_or(&[][..], |list| list) {
            let previous = at(*previous);
            if !allowed(previous) {
                continue;
            }
            if let Some(mark) = marked.get_mut(previous)
                && !*mark
            {
                *mark = true;
                queue.push_back(previous);
            }
        }
    }
    marked
}

fn flag(flags: &[bool], state: u32) -> bool {
    flags.get(at(state)).copied().unwrap_or(false)
}

fn liveness(
    model: &Model,
    graph: &Graph,
    property: &CompiledProperty,
    predecessors: &[Vec<u32>],
) -> Result<Option<String>> {
    let goal = evaluate(model, graph, &property.condition)?;
    let region: Vec<bool> = goal.iter().map(|goal| !goal).collect();
    let starts: Vec<bool> = match &property.premise {
        Some(premise) => evaluate(model, graph, premise)?,
        None => (0..graph.states.len()).map(|state| state == 0).collect(),
    };
    if !starts.iter().any(|start| *start) {
        return Ok(Some(format!(
            "'{}' holds vacuously: its 'whenever' condition is never true",
            property.text
        )));
    }

    let fair = fair_calls(model);
    let component_of = fair_components(graph, &fair, &region);
    let mut doomed: Vec<bool> = component_of
        .iter()
        .map(|component| *component != UNVISITED)
        .collect();
    for (state, in_region) in region.iter().enumerate() {
        if *in_region
            && graph.out(index32(state)).is_empty()
            && let Some(mark) = doomed.get_mut(state)
        {
            *mark = true;
        }
    }
    let can_fail = backward(&doomed, predecessors, |state| {
        region.get(state).copied().unwrap_or(false)
    });
    let Some(start) = (0..graph.states.len())
        .find(|state| flag(&starts, index32(*state)) && flag(&can_fail, index32(*state)))
    else {
        return Ok(None);
    };

    let start = index32(start);
    let mut path = graph.path_to(start);
    let to_doom = route(
        graph,
        start,
        |state| flag(&region, state),
        |state| flag(&doomed, state),
        false,
    );
    let end = to_doom.last().map_or(start, |edge| edge.to);
    path.extend(to_doom);
    if graph.out(end).is_empty() {
        return Err(Error::of(
            ErrorKind::Violation,
            Some(property.line),
            format!(
                "'{}' fails: the system can stop before it happens",
                property.text
            ),
        )
        .with_trace(graph.trace(model, &path, None)));
    }
    let component = component_of.get(at(end)).copied().unwrap_or(UNVISITED);
    let loop_from = path.len();
    path.extend(fair_cycle(graph, &fair, &component_of, component, end));
    Err(Error::of(
        ErrorKind::Violation,
        Some(property.line),
        format!(
            "'{}' fails: the system can run forever without it happening",
            property.text
        ),
    )
    .with_trace(graph.trace(model, &path, Some(loop_from))))
}

fn fair_calls(model: &Model) -> Vec<Fairness> {
    model
        .calls
        .iter()
        .map(|call| {
            model
                .actions
                .get(call.action)
                .map_or(Fairness::Unfair, |action| action.fairness)
        })
        .collect()
}

fn fairness(fair: &[Fairness], call: u32) -> Fairness {
    fair.get(at(call)).copied().unwrap_or(Fairness::Unfair)
}

/// The states of `region` that lie on a fair cycle inside it, grouped into
/// strongly connected components (`UNVISITED` for every other state).
///
/// A component holds a fair cycle when it has an internal edge, every weakly
/// fair call enabled in all of its states is taken inside it, and every
/// strongly fair call enabled anywhere in it is taken inside it. A component
/// that fails only the strong condition may still contain a fair cycle that
/// avoids the states enabling the offending calls, so those states are
/// removed and the rest is decomposed again (Emerson and Lei). Each round
/// removes at least one state, and a component's set of enabled strongly fair
/// calls shrinks on every round it survives, so this terminates quickly.
fn fair_components(graph: &Graph, fair: &[Fairness], region: &[bool]) -> Vec<u32> {
    let mut alive = region.to_vec();
    loop {
        let component_of = components(graph, &alive);
        let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
        for (state, component) in component_of.iter().enumerate() {
            if *component != UNVISITED {
                members.entry(*component).or_default().push(index32(state));
            }
        }
        let mut changed = false;
        for (component, states) in &members {
            let usage = usage(graph, fair, &component_of, *component, states);
            let weak_starved = usage.enabled.iter().any(|(call, enabled)| {
                fairness(fair, *call) == Fairness::Weak
                    && *enabled == states.len()
                    && !usage.taken.contains_key(call)
            });
            let removed: Vec<u32> = if usage.internal_edges == 0 || weak_starved {
                states.clone()
            } else {
                let starved: Vec<u32> = usage
                    .enabled
                    .keys()
                    .filter(|call| {
                        fairness(fair, **call) == Fairness::Strong
                            && !usage.taken.contains_key(call)
                    })
                    .copied()
                    .collect();
                states
                    .iter()
                    .filter(|state| {
                        starved
                            .iter()
                            .any(|call| graph.target(**state, *call).is_some())
                    })
                    .copied()
                    .collect()
            };
            for state in removed {
                if let Some(mark) = alive.get_mut(at(state)) {
                    *mark = false;
                    changed = true;
                }
            }
        }
        if !changed {
            return component_of;
        }
    }
}

fn components(graph: &Graph, region: &[bool]) -> Vec<u32> {
    let count = graph.states.len();
    let mut order = vec![UNVISITED; count];
    let mut low = vec![UNVISITED; count];
    let mut component = vec![UNVISITED; count];
    let mut on_stack = vec![false; count];
    let mut stack: Vec<u32> = Vec::new();
    let mut work: Vec<(u32, usize)> = Vec::new();
    let mut next_order: u32 = 0;
    let mut next_component: u32 = 0;

    for root in 0..count {
        if !region.get(root).copied().unwrap_or(false)
            || order.get(root).copied() != Some(UNVISITED)
        {
            continue;
        }
        work.push((index32(root), 0));
        while let Some((state, position)) = work.last().copied() {
            let here = at(state);
            if position == 0 {
                if let (Some(order), Some(low)) = (order.get_mut(here), low.get_mut(here)) {
                    *order = next_order;
                    *low = next_order;
                }
                next_order += 1;
                stack.push(state);
                if let Some(mark) = on_stack.get_mut(here) {
                    *mark = true;
                }
            }
            if let Some(edge) = graph.out(state).get(position) {
                if let Some(top) = work.last_mut() {
                    top.1 += 1;
                }
                let to = at(edge.to);
                if !region.get(to).copied().unwrap_or(false) {
                    continue;
                }
                let to_order = order.get(to).copied().unwrap_or(UNVISITED);
                if to_order == UNVISITED {
                    work.push((edge.to, 0));
                } else if on_stack.get(to).copied().unwrap_or(false)
                    && let Some(low) = low.get_mut(here)
                {
                    *low = (*low).min(to_order);
                }
                continue;
            }
            work.pop();
            let here_low = low.get(here).copied().unwrap_or(UNVISITED);
            if let Some((parent, _)) = work.last()
                && let Some(low) = low.get_mut(at(*parent))
            {
                *low = (*low).min(here_low);
            }
            if order.get(here).copied() == Some(here_low) {
                while let Some(member) = stack.pop() {
                    if let Some(mark) = on_stack.get_mut(at(member)) {
                        *mark = false;
                    }
                    if let Some(slot) = component.get_mut(at(member)) {
                        *slot = next_component;
                    }
                    if member == state {
                        break;
                    }
                }
                next_component += 1;
            }
        }
    }
    component
}

struct Usage {
    internal_edges: usize,
    enabled: HashMap<u32, usize>,
    taken: HashMap<u32, Edge>,
    taken_from: HashMap<u32, u32>,
}

fn usage(
    graph: &Graph,
    fair: &[Fairness],
    component_of: &[u32],
    component: u32,
    states: &[u32],
) -> Usage {
    let mut usage = Usage {
        internal_edges: 0,
        enabled: HashMap::new(),
        taken: HashMap::new(),
        taken_from: HashMap::new(),
    };
    for state in states {
        for edge in graph.out(*state) {
            let internal = component_of.get(at(edge.to)).copied() == Some(component);
            if internal {
                usage.internal_edges += 1;
            }
            if fairness(fair, edge.call) != Fairness::Unfair {
                *usage.enabled.entry(edge.call).or_default() += 1;
                if internal && !usage.taken.contains_key(&edge.call) {
                    usage.taken.insert(edge.call, *edge);
                    usage.taken_from.insert(edge.call, *state);
                }
            }
        }
    }
    usage
}

fn route(
    graph: &Graph,
    from: u32,
    inside: impl Fn(u32) -> bool,
    accept: impl Fn(u32) -> bool,
    must_move: bool,
) -> Vec<Edge> {
    if !must_move && accept(from) {
        return Vec::new();
    }
    let mut parents: HashMap<u32, (u32, Edge)> = HashMap::new();
    let mut queue = VecDeque::from([from]);
    let mut found = None;
    'search: while let Some(state) = queue.pop_front() {
        for edge in graph.out(state) {
            if !inside(edge.to) || parents.contains_key(&edge.to) {
                continue;
            }
            parents.insert(edge.to, (state, *edge));
            if accept(edge.to) {
                found = Some(edge.to);
                break 'search;
            }
            if edge.to != from {
                queue.push_back(edge.to);
            }
        }
    }
    let mut path = Vec::new();
    let mut current = found;
    while let Some(state) = current {
        let Some((previous, edge)) = parents.get(&state) else {
            break;
        };
        path.push(*edge);
        current = if *previous == from {
            None
        } else {
            Some(*previous)
        };
    }
    path.reverse();
    path
}

fn fair_cycle(
    graph: &Graph,
    fair: &[Fairness],
    component_of: &[u32],
    component: u32,
    entry: u32,
) -> Vec<Edge> {
    let states: Vec<u32> = component_of
        .iter()
        .enumerate()
        .filter(|(_, id)| **id == component)
        .map(|(state, _)| index32(state))
        .collect();
    let usage = usage(graph, fair, component_of, component, &states);
    let inside = |state: u32| component_of.get(at(state)).copied() == Some(component);
    let mut cycle: Vec<Edge> = Vec::new();
    let mut current = entry;
    let mut calls: Vec<u32> = usage.enabled.keys().copied().collect();
    calls.sort_unstable();
    for call in calls {
        let disabled_at = |state: u32| graph.target(state, call).is_none();
        let strong = fairness(fair, call) == Fairness::Strong;
        let satisfied = cycle.iter().any(|edge| edge.call == call)
            || !strong && (disabled_at(entry) || cycle.iter().any(|edge| disabled_at(edge.to)));
        if satisfied {
            continue;
        }
        let everywhere = usage.enabled.get(&call).copied() == Some(states.len());
        if everywhere || strong {
            let (Some(edge), Some(from)) = (usage.taken.get(&call), usage.taken_from.get(&call))
            else {
                continue;
            };
            let from = *from;
            cycle.extend(route(graph, current, inside, |state| state == from, false));
            cycle.push(*edge);
            current = edge.to;
        } else {
            let leg = route(graph, current, inside, disabled_at, false);
            current = leg.last().map_or(current, |edge| edge.to);
            cycle.extend(leg);
        }
    }
    let must_move = cycle.is_empty();
    cycle.extend(route(
        graph,
        current,
        inside,
        |state| state == entry,
        must_move,
    ));
    cycle
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, bound: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % bound
        }
    }

    fn random_graph(rng: &mut Rng, states: usize, calls: u32) -> Graph {
        let mut graph = Graph {
            states: (0..states).map(|_| Box::from([])).collect(),
            parents: vec![None; states],
            edges: Vec::new(),
            first_edge: Vec::new(),
            index: HashMap::new(),
        };
        for _ in 0..states {
            graph.first_edge.push(graph.edges.len());
            for call in 0..calls {
                if rng.below(2) == 0 {
                    let to = index32(usize::try_from(rng.below(states as u64)).unwrap());
                    graph.edges.push(Edge { call, to });
                }
            }
        }
        graph.first_edge.push(graph.edges.len());
        graph
    }

    /// Whether some run stays in exactly `subset` forever while respecting
    /// fairness: the subset is strongly connected through its own edges, and
    /// the run may take every one of those edges infinitely often.
    fn fair_subset(graph: &Graph, fair: &[Fairness], subset: &[bool]) -> bool {
        let members: Vec<u32> = (0..subset.len())
            .filter(|state| subset[*state])
            .map(index32)
            .collect();
        let inside = |state: u32| subset[at(state)];
        let Some(first) = members.first() else {
            return false;
        };
        let internal: Vec<Edge> = members
            .iter()
            .flat_map(|state| graph.out(*state).iter().copied())
            .filter(|edge| inside(edge.to))
            .collect();
        if internal.is_empty() {
            return false;
        }
        let reach = |from: u32, forward: bool| {
            let mut seen = vec![false; subset.len()];
            let mut stack = vec![from];
            seen[at(from)] = true;
            while let Some(state) = stack.pop() {
                for other in &members {
                    let (a, b) = if forward {
                        (state, *other)
                    } else {
                        (*other, state)
                    };
                    if !seen[at(*other)] && graph.out(a).iter().any(|edge| edge.to == b) {
                        seen[at(*other)] = true;
                        stack.push(*other);
                    }
                }
            }
            members.iter().all(|state| seen[at(*state)])
        };
        if !reach(*first, true) || !reach(*first, false) {
            return false;
        }
        (0..index32(fair.len())).all(|call| {
            let taken = internal.iter().any(|edge| edge.call == call);
            let enabled = |state: &u32| graph.target(*state, call).is_some();
            match fair[at(call)] {
                Fairness::Unfair => true,
                Fairness::Weak => taken || !members.iter().all(enabled),
                Fairness::Strong => taken || !members.iter().any(enabled),
            }
        })
    }

    #[test]
    fn fair_components_match_brute_force_over_every_subset() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..3000 {
            let states = usize::try_from(rng.below(7) + 1).unwrap();
            let calls = u32::try_from(rng.below(4) + 1).unwrap();
            let graph = random_graph(&mut rng, states, calls);
            let fair: Vec<Fairness> = (0..calls)
                .map(|_| match rng.below(3) {
                    0 => Fairness::Unfair,
                    1 => Fairness::Weak,
                    _ => Fairness::Strong,
                })
                .collect();
            let region: Vec<bool> = (0..states).map(|_| rng.below(4) != 0).collect();

            let mut expected = vec![false; states];
            for mask in 1u32..(1 << states) {
                let subset: Vec<bool> = (0..states).map(|state| mask & (1 << state) != 0).collect();
                if subset
                    .iter()
                    .zip(&region)
                    .any(|(member, allowed)| *member && !allowed)
                {
                    continue;
                }
                if fair_subset(&graph, &fair, &subset) {
                    for (state, member) in subset.iter().enumerate() {
                        expected[state] |= *member;
                    }
                }
            }
            let component_of = fair_components(&graph, &fair, &region);
            let actual: Vec<bool> = component_of.iter().map(|c| *c != UNVISITED).collect();
            assert_eq!(actual, expected, "fairness {fair:?}, region {region:?}");

            for (state, member) in actual.iter().enumerate() {
                if !member {
                    continue;
                }
                let entry = index32(state);
                let component = component_of[state];
                let cycle = fair_cycle(&graph, &fair, &component_of, component, entry);
                assert!(!cycle.is_empty());
                assert_eq!(cycle.last().map(|edge| edge.to), Some(entry));
                let mut on_cycle = vec![false; states];
                let mut current = entry;
                for edge in &cycle {
                    assert!(graph.out(current).contains(edge), "{cycle:?} is not a path");
                    current = edge.to;
                    on_cycle[at(current)] = true;
                }
                let visited: Vec<u32> = (0..states).filter(|s| on_cycle[*s]).map(index32).collect();
                for call in 0..calls {
                    let taken = cycle.iter().any(|edge| edge.call == call);
                    let enabled = |state: &u32| graph.target(*state, call).is_some();
                    let ok = match fair[at(call)] {
                        Fairness::Unfair => true,
                        Fairness::Weak => taken || !visited.iter().all(enabled),
                        Fairness::Strong => taken || !visited.iter().any(enabled),
                    };
                    assert!(ok, "cycle {cycle:?} starves call {call}, fairness {fair:?}");
                }
            }
        }
    }
}
