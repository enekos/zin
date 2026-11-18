use crate::ast::PropertyKind;
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
    let component_of = components(graph, &region);
    let mut members: HashMap<u32, Vec<u32>> = HashMap::new();
    for (state, component) in component_of.iter().enumerate() {
        if *component != UNVISITED {
            members.entry(*component).or_default().push(index32(state));
        }
    }
    let mut doomed = vec![false; graph.states.len()];
    for (state, in_region) in region.iter().enumerate() {
        if *in_region
            && graph.out(index32(state)).is_empty()
            && let Some(mark) = doomed.get_mut(state)
        {
            *mark = true;
        }
    }
    for (component, states) in &members {
        if admits_fair_cycle(graph, &fair, &component_of, *component, states) {
            for state in states {
                if let Some(mark) = doomed.get_mut(at(*state)) {
                    *mark = true;
                }
            }
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

fn fair_calls(model: &Model) -> Vec<bool> {
    model
        .calls
        .iter()
        .map(|call| {
            model
                .actions
                .get(call.action)
                .is_some_and(|action| action.fair)
        })
        .collect()
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
    fair: &[bool],
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
            if flag(fair, edge.call) {
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

fn admits_fair_cycle(
    graph: &Graph,
    fair: &[bool],
    component_of: &[u32],
    component: u32,
    states: &[u32],
) -> bool {
    let usage = usage(graph, fair, component_of, component, states);
    usage.internal_edges > 0
        && usage
            .enabled
            .iter()
            .all(|(call, enabled)| *enabled < states.len() || usage.taken.contains_key(call))
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
    fair: &[bool],
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
        let satisfied = cycle.iter().any(|edge| edge.call == call)
            || disabled_at(entry)
            || cycle.iter().any(|edge| disabled_at(edge.to));
        if satisfied {
            continue;
        }
        let everywhere = usage.enabled.get(&call).copied() == Some(states.len());
        if everywhere {
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
