use crate::ast::{PropertyKind, Value};
use crate::check::{ScenarioEvent, Verification};
use crate::explore::Graph;
use crate::json::Json;
use crate::model::{Domain, FieldType, Model, at, index32};
use crate::observe::Classes;
use crate::{Error, ErrorKind, Result};
use std::collections::{BTreeSet, HashMap, VecDeque};

mod identify;
mod pairs;

pub const PLAN_FORMAT: &str = "plan/1";
/// The format of a plan whose state holds records or nested maps. Plans for
/// specifications without them stay `plan/1`, byte for byte.
pub const PLAN_FORMAT_2: &str = "plan/2";
const DEFAULT_MAX_STEPS: usize = 200;
const MAX_PLAN_STEPS: usize = 2_000_000;
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cover {
    Transitions,
    States,
    Actions,
    Scenarios,
    /// Every feasible pair of adjacent calls from each reachable state, or a
    /// deterministic sample of `limit` of them.
    Pairs {
        limit: Option<usize>,
    },
}

impl Cover {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Transitions => "transitions",
            Self::States => "states",
            Self::Actions => "actions",
            Self::Scenarios => "scenarios",
            Self::Pairs { .. } => "pairs",
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "transitions" => Ok(Self::Transitions),
            "states" => Ok(Self::States),
            "actions" => Ok(Self::Actions),
            "scenarios" => Ok(Self::Scenarios),
            "pairs" => Ok(Self::Pairs { limit: None }),
            other => Err(Error::usage(format!(
                "unknown coverage '{other}'; use transitions, states, actions, scenarios, or pairs"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlanOptions {
    pub cover: Cover,
    /// Soft target for tour length, excluding the shortest setup to the current state.
    pub max_steps: usize,
    /// Hard limit on all emitted replay steps.
    pub step_budget: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            cover: Cover::Transitions,
            max_steps: DEFAULT_MAX_STEPS,
            step_budget: MAX_PLAN_STEPS,
        }
    }
}

impl PlanOptions {
    #[must_use]
    pub fn cover(cover: Cover) -> Self {
        Self {
            cover,
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<()> {
        if self.max_steps == 0 || self.step_budget == 0 || self.step_budget > MAX_PLAN_STEPS {
            return Err(Error::usage(format!(
                "max_steps must be positive and step_budget must be in 1..={MAX_PLAN_STEPS}"
            )));
        }
        if let Cover::Pairs { limit: Some(limit) } = self.cover
            && (limit == 0
                || !u64::try_from(limit).is_ok_and(|n| n <= MAX_SAFE_INTEGER.unsigned_abs()))
        {
            return Err(Error::usage(
                "a pair limit must be a positive JavaScript safe integer",
            ));
        }
        if !u64::try_from(self.max_steps).is_ok_and(|n| n <= MAX_SAFE_INTEGER.unsigned_abs()) {
            return Err(Error::usage(
                "plan options must fit in a JavaScript safe integer",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TestKind {
    Scenario,
    Witness,
    Tour,
    /// Confirms which state a transition reached; see [`Plan::build_observing`].
    Identify,
}

impl TestKind {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Scenario => "scenario",
            Self::Witness => "witness",
            Self::Tour => "tour",
            Self::Identify => "identify",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanStep {
    pub call: u32,
    pub to: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct PlanTest {
    pub name: String,
    pub kind: TestKind,
    pub steps: Vec<PlanStep>,
}

/// What the plan's generated tests account for, beyond the requested cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Coverage {
    Obligations,
    Pairs {
        /// Feasible (state, first call, second call) obligations in the model.
        eligible: u64,
        /// Distinct adjacent pairs the plan exercises, including setup steps.
        covered: usize,
    },
}

/// What a plan built by [`Plan::build_observing`] assumed and added.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Identification {
    /// The fields the adapter's `observe` is taken to return, in declaration order.
    pub fields: Vec<String>,
    /// Distinct transitions the cover's tests take.
    pub transitions: usize,
    /// Of those, the ones the cover's tests could not tell from a transition
    /// into another state with the same observation.
    pub unconfirmed: usize,
    /// Tests added to confirm them and the initial state.
    pub tests: usize,
    /// States no test can tell from another; see [`crate::adequacy`].
    pub blind_states: usize,
}

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Plan {
    pub cover: Cover,
    pub coverage: Coverage,
    pub max_steps: usize,
    pub step_budget: usize,
    pub tests: Vec<PlanTest>,
    pub accepted_steps: usize,
    pub rejected_steps: usize,
    /// Present when the plan identifies states under a stated observation.
    pub identification: Option<Identification>,
}

struct Pending {
    edges: Vec<bool>,
    refusals: Vec<Vec<u32>>,
    at_state: Vec<usize>,
    total: usize,
}

impl Pending {
    fn new(graph: &Graph) -> Self {
        Self {
            edges: vec![false; graph.transition_count()],
            refusals: vec![Vec::new(); graph.state_count()],
            at_state: vec![0; graph.state_count()],
            total: 0,
        }
    }

    fn add(&mut self, state: u32) {
        if let Some(count) = self.at_state.get_mut(at(state)) {
            *count += 1;
        }
        self.total += 1;
    }

    fn done(&mut self, state: u32) {
        if let Some(count) = self.at_state.get_mut(at(state)) {
            *count = count.saturating_sub(1);
        }
        self.total = self.total.saturating_sub(1);
    }

    fn need_edge(&mut self, state: u32, edge: usize) {
        if let Some(needed) = self.edges.get_mut(edge)
            && !*needed
        {
            *needed = true;
            self.add(state);
        }
    }

    fn need_refusal(&mut self, state: u32, call: u32) {
        if let Some(list) = self.refusals.get_mut(at(state)) {
            list.push(call);
        }
        self.add(state);
    }

    fn cover_edge(&mut self, state: u32, edge: usize) -> bool {
        match self.edges.get_mut(edge) {
            Some(needed) if *needed => {
                *needed = false;
                self.done(state);
                true
            }
            _ => false,
        }
    }

    fn cover_refusal(&mut self, state: u32, call: u32) {
        let Some(list) = self.refusals.get_mut(at(state)) else {
            return;
        };
        if let Some(position) = list.iter().position(|pending| *pending == call) {
            list.swap_remove(position);
            self.done(state);
        }
    }

    fn take_refusals(&mut self, state: u32) -> Vec<u32> {
        let mut calls = self
            .refusals
            .get_mut(at(state))
            .map(std::mem::take)
            .unwrap_or_default();
        calls.sort_unstable();
        for _ in &calls {
            self.done(state);
        }
        calls
    }

    fn waiting(&self, state: u32) -> bool {
        self.at_state.get(at(state)).copied().unwrap_or(0) > 0
    }
}

fn edge_index(graph: &Graph, state: u32, call: u32) -> Option<usize> {
    let start = graph.first_edge.get(at(state)).copied()?;
    graph
        .out(state)
        .binary_search_by_key(&call, |edge| edge.call)
        .ok()
        .map(|position| start + position)
}

fn pending_for(options: &PlanOptions, model: &Model, graph: &Graph) -> Result<Pending> {
    let mut pending = Pending::new(graph);
    let calls = index32(model.calls.len());
    match options.cover {
        Cover::Scenarios | Cover::Pairs { .. } => {}
        Cover::Transitions => {
            let refusals = graph
                .state_count()
                .saturating_mul(model.calls.len())
                .saturating_sub(graph.transition_count());
            if graph.transition_count().saturating_add(refusals) > options.step_budget {
                return Err(Error::of(
                    ErrorKind::Limit,
                    None,
                    format!(
                        "covering every transition and refusal needs more than {} steps; use --cover states or --cover actions",
                        options.step_budget
                    ),
                ));
            }
            for state in 0..graph.state_count() {
                let state = index32(state);
                let start = graph.first_edge.get(at(state)).copied().unwrap_or(0);
                let out = graph.out(state);
                for position in 0..out.len() {
                    pending.need_edge(state, start + position);
                }
                let mut enabled = out.iter().map(|edge| edge.call).peekable();
                for call in 0..calls {
                    if enabled.peek() == Some(&call) {
                        enabled.next();
                    } else {
                        pending.need_refusal(state, call);
                    }
                }
            }
        }
        Cover::States => {
            for parent in graph.parents.iter().flatten() {
                if let Some(edge) = edge_index(graph, parent.0, parent.1) {
                    pending.need_edge(parent.0, edge);
                }
            }
        }
        Cover::Actions => {
            let mut accepted = vec![false; model.actions.len()];
            let mut refused = vec![false; model.actions.len()];
            for state in 0..graph.state_count() {
                let state = index32(state);
                let start = graph.first_edge.get(at(state)).copied().unwrap_or(0);
                let out = graph.out(state);
                for (position, edge) in out.iter().enumerate() {
                    let action = model.calls.get(at(edge.call)).map_or(0, |call| call.action);
                    if let Some(seen) = accepted.get_mut(action)
                        && !*seen
                    {
                        *seen = true;
                        pending.need_edge(state, start + position);
                    }
                }
                if refused.iter().all(|seen| *seen) {
                    continue;
                }
                let mut enabled = out.iter().map(|edge| edge.call).peekable();
                for call in 0..calls {
                    if enabled.peek() == Some(&call) {
                        enabled.next();
                        continue;
                    }
                    let action = model.calls.get(at(call)).map_or(0, |call| call.action);
                    if let Some(seen) = refused.get_mut(action)
                        && !*seen
                    {
                        *seen = true;
                        pending.need_refusal(state, call);
                    }
                }
            }
        }
    }
    Ok(pending)
}

fn route_to_pending(graph: &Graph, waiting: impl Fn(u32) -> bool, from: u32) -> Vec<(u32, usize)> {
    let mut parents: HashMap<u32, (u32, usize)> = HashMap::new();
    let mut queue = VecDeque::from([from]);
    let mut found = None;
    'search: while let Some(state) = queue.pop_front() {
        let start = graph.first_edge.get(at(state)).copied().unwrap_or(0);
        for (position, edge) in graph.out(state).iter().enumerate() {
            if edge.to == from || parents.contains_key(&edge.to) {
                continue;
            }
            parents.insert(edge.to, (state, start + position));
            if waiting(edge.to) {
                found = Some(edge.to);
                break 'search;
            }
            queue.push_back(edge.to);
        }
    }
    let mut path = Vec::new();
    let mut current = found;
    while let Some(state) = current {
        let Some((previous, edge)) = parents.get(&state) else {
            break;
        };
        path.push((*previous, *edge));
        current = Some(*previous).filter(|previous| *previous != from);
    }
    path.reverse();
    path
}

struct Budget {
    remaining: usize,
    limit: usize,
}

impl Budget {
    fn new(limit: usize) -> Self {
        Self {
            remaining: limit,
            limit,
        }
    }

    fn charge(&mut self, steps: usize) -> Result<()> {
        self.remaining = self.remaining.checked_sub(steps).ok_or_else(|| {
            Error::of(
                ErrorKind::Limit,
                None,
                format!(
                    "plan exceeds the total replay budget of {} steps; reduce coverage or raise --step-budget",
                    self.limit
                ),
            )
        })?;
        Ok(())
    }
}

fn access_depths(graph: &Graph) -> Vec<usize> {
    let mut depths = vec![0; graph.state_count()];
    // A BFS parent is always discovered before its child.
    for (state, parent) in graph.parents.iter().enumerate() {
        if let Some((from, _)) = parent {
            depths[state] = depths[at(*from)] + 1;
        }
    }
    depths
}

fn tours(
    graph: &Graph,
    pending: &mut Pending,
    max_steps: usize,
    depths: &[usize],
    budget: &mut Budget,
) -> Result<Vec<Vec<PlanStep>>> {
    let mut tours = Vec::new();
    while pending.total > 0 {
        let mut steps = Vec::new();
        let mut state = 0;
        let initial_pending = pending.total;
        loop {
            let refused = pending.take_refusals(state);
            budget.charge(refused.len())?;
            steps.extend(refused.into_iter().map(|call| PlanStep { call, to: None }));
            let start = graph.first_edge[at(state)];
            if let Some((position, edge)) = graph
                .out(state)
                .iter()
                .enumerate()
                .find(|(position, _)| pending.edges[start + position])
            {
                budget.charge(1)?;
                pending.cover_edge(state, start + position);
                steps.push(PlanStep {
                    call: edge.call,
                    to: Some(edge.to),
                });
                state = edge.to;
            } else if pending.total > 0 {
                let route = route_to_pending(graph, |state| pending.waiting(state), state);
                let Some((_, last)) = route.last() else { break };
                let destination = graph.edges[*last].to;
                // Restart only when a fresh setup is cheaper than continuing.
                if pending.total < initial_pending
                    && steps.len() + route.len()
                        >= max_steps.saturating_add(depths[at(destination)])
                {
                    break;
                }
                budget.charge(route.len())?;
                for (from, index) in route {
                    let edge = graph.edges[index];
                    pending.cover_edge(from, index);
                    steps.push(PlanStep {
                        call: edge.call,
                        to: Some(edge.to),
                    });
                    state = edge.to;
                }
            }
            if pending.total == 0
                || (pending.total < initial_pending
                    && steps.len().saturating_sub(depths[at(state)]) >= max_steps)
            {
                break;
            }
        }
        if initial_pending == pending.total {
            return Err(Error::new(
                "planner could not reach its remaining coverage obligations",
            ));
        }
        tours.push(steps);
    }
    Ok(tours)
}

impl Plan {
    pub fn build(verification: &Verification, options: &PlanOptions) -> Result<Self> {
        Self::assemble(verification, options, None)
    }

    /// Like [`Plan::build`], then adds `identify` tests so that every
    /// transition the plan takes is confirmed to reach the specified state
    /// by a runner whose adapter observes exactly `fields` (any subset of
    /// the state fields; none means only accepted and rejected calls are
    /// seen). After each such transition some test continues with steps that
    /// every state with the same observation but different behavior would
    /// answer differently.
    pub fn build_observing(
        verification: &Verification,
        options: &PlanOptions,
        fields: &[String],
    ) -> Result<Self> {
        let classes = Classes::analyse(verification, fields)?;
        Self::assemble(verification, options, Some(&classes))
    }

    fn assemble(
        verification: &Verification,
        options: &PlanOptions,
        observing: Option<&Classes>,
    ) -> Result<Self> {
        let model = &verification.model;
        let graph = &verification.graph;
        if graph.reduced() {
            return Err(Error::usage(
                "conformance plans need concrete states; build them from verify(), which explores the full model",
            ));
        }
        options.validate()?;
        check_safe_integers(model)?;
        let mut pending = pending_for(options, model, graph)?;
        let mut budget = Budget::new(options.step_budget);
        let mut tests = Vec::new();

        for scenario in &verification.scenarios {
            let steps: Vec<PlanStep> = scenario
                .events
                .iter()
                .filter_map(|event| match event {
                    ScenarioEvent::Call { call, to } => Some(PlanStep {
                        call: *call,
                        to: Some(*to),
                    }),
                    ScenarioEvent::Reject { call } => Some(PlanStep {
                        call: *call,
                        to: None,
                    }),
                    ScenarioEvent::Expect { .. } => None,
                })
                .collect();
            budget.charge(steps.len())?;
            tests.push(PlanTest {
                name: format!("scenario: {}", scenario.name),
                kind: TestKind::Scenario,
                steps,
            });
        }
        for property in verification
            .properties
            .iter()
            .filter(|_| options.cover != Cover::Scenarios)
        {
            let (PropertyKind::Sometimes, Some(witness)) = (property.kind, property.witness) else {
                continue;
            };
            let steps: Vec<PlanStep> = graph
                .path_to(witness)
                .into_iter()
                .map(|edge| PlanStep {
                    call: edge.call,
                    to: Some(edge.to),
                })
                .collect();
            budget.charge(steps.len())?;
            if !steps.is_empty() {
                tests.push(PlanTest {
                    name: format!("reaches: {}", property.text),
                    kind: TestKind::Witness,
                    steps,
                });
            }
        }
        for test in &tests {
            let mut state: u32 = 0;
            for step in &test.steps {
                match step.to {
                    Some(to) => {
                        if let Some(edge) = edge_index(graph, state, step.call) {
                            pending.cover_edge(state, edge);
                        }
                        state = to;
                    }
                    None => pending.cover_refusal(state, step.call),
                }
            }
        }
        let depths = access_depths(graph);
        let (generated, coverage) = match options.cover {
            Cover::Pairs { limit } => {
                let calls = index32(model.calls.len());
                let eligible = pairs::eligible(graph, calls)?;
                let wanted = limit.map_or(eligible, |n| eligible.min(n as u64));
                let target = usize::try_from(wanted)
                    .ok()
                    .filter(|n| *n <= options.step_budget)
                    .ok_or_else(|| {
                        Error::of(
                            ErrorKind::Limit,
                            None,
                            format!(
                                "covering {wanted} adjacent-call pairs needs more than {} replay steps; use --pair-limit to sample pairs",
                                options.step_budget
                            ),
                        )
                    })?;
                let mut selected = pairs::Pairs::select(graph, calls, target);
                for test in &tests {
                    selected.record(&test.steps);
                }
                let tours = pairs::pair_tours(
                    graph,
                    &mut selected,
                    options.max_steps,
                    &depths,
                    &mut budget,
                )?;
                let coverage = Coverage::Pairs {
                    eligible,
                    covered: selected.covered.len(),
                };
                (tours, coverage)
            }
            _ => (
                tours(graph, &mut pending, options.max_steps, &depths, &mut budget)?,
                Coverage::Obligations,
            ),
        };
        for (index, steps) in generated.into_iter().enumerate() {
            tests.push(PlanTest {
                name: format!("tour {}", index + 1),
                kind: TestKind::Tour,
                steps,
            });
        }
        // An empty plan still checks the initial observation.
        if tests.is_empty() && options.cover != Cover::Scenarios {
            tests.push(PlanTest {
                name: "initial state".into(),
                kind: TestKind::Tour,
                steps: Vec::new(),
            });
        }
        let identification = match observing {
            Some(classes) => {
                let before = tests.len();
                let identified = identify::identify(graph, classes, &mut tests, &mut budget)?;
                Some(Identification {
                    fields: classes.fields.clone(),
                    transitions: identified.transitions,
                    unconfirmed: identified.unconfirmed,
                    tests: tests.len() - before,
                    blind_states: classes.blind_states(),
                })
            }
            None => None,
        };
        let accepted_steps = tests
            .iter()
            .flat_map(|test| &test.steps)
            .filter(|step| step.to.is_some())
            .count();
        let total: usize = tests.iter().map(|test| test.steps.len()).sum();
        Ok(Self {
            cover: options.cover,
            coverage,
            max_steps: options.max_steps,
            step_budget: options.step_budget,
            tests,
            accepted_steps,
            rejected_steps: total - accepted_steps,
            identification,
        })
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let pairs = match self.coverage {
            Coverage::Pairs { eligible, covered } => format!(", {covered} of {eligible} pairs"),
            Coverage::Obligations => String::new(),
        };
        let observing = self.identification.as_ref().map_or_else(String::new, |id| {
            let fields = if id.fields.is_empty() {
                "no fields".to_string()
            } else {
                id.fields.join(", ")
            };
            format!(
                "; observing {fields}: {} for {} of {} transitions",
                crate::counted(id.tests, "identify test", "identify tests"),
                id.unconfirmed,
                id.transitions
            )
        });
        format!(
            "{} tests, {} accepted steps, {} rejected steps (cover: {}{pairs}{observing})",
            self.tests.len(),
            self.accepted_steps,
            self.rejected_steps,
            self.cover.name()
        )
    }

    #[must_use]
    pub fn to_json(&self, verification: &Verification) -> Json {
        let model = &verification.model;
        let graph = &verification.graph;
        let mut used: BTreeSet<u32> = BTreeSet::from([0]);
        for step in self.tests.iter().flat_map(|test| &test.steps) {
            let mut current = step.to;
            while let Some(state) = current {
                if !used.insert(state) {
                    break;
                }
                current = graph
                    .parents
                    .get(at(state))
                    .copied()
                    .flatten()
                    .map(|(from, _)| from);
            }
        }
        let renumbered: HashMap<u32, usize> = used
            .iter()
            .enumerate()
            .map(|(new, old)| (*old, new))
            .collect();
        let renumber = |state: u32| Json::count(renumbered.get(&state).copied().unwrap_or(0));

        let states = used
            .iter()
            .map(|state| state_json(&verification.state(*state)))
            .collect();
        let parents = used
            .iter()
            .map(
                |state| match graph.parents.get(at(*state)).copied().flatten() {
                    Some((from, call)) => {
                        Json::Array(vec![renumber(from), Json::Int(i64::from(call))])
                    }
                    None => Json::Null,
                },
            )
            .collect();
        let calls = (0..model.calls.len())
            .map(|call| {
                let call = index32(call);
                Json::object([
                    ("action", Json::str(model.call_action(call))),
                    (
                        "args",
                        Json::Array(model.call_values(call).iter().map(value_json).collect()),
                    ),
                    ("text", Json::str(model.call_text(call))),
                ])
            })
            .collect();
        let tests = self
            .tests
            .iter()
            .map(|test| {
                Json::object([
                    ("name", Json::str(test.name.clone())),
                    ("kind", Json::str(test.kind.name())),
                    (
                        "steps",
                        Json::Array(
                            test.steps
                                .iter()
                                .map(|step| {
                                    Json::Array(vec![
                                        Json::Int(i64::from(step.call)),
                                        step.to.map_or(Json::Int(-1), renumber),
                                    ])
                                })
                                .collect(),
                        ),
                    ),
                ])
            })
            .collect();
        let mut plan = Json::object([
            (
                "zin",
                Json::str(
                    if model.fields.iter().all(|field| field.kind.is_classic()) {
                        PLAN_FORMAT
                    } else {
                        PLAN_FORMAT_2
                    },
                ),
            ),
            ("system", Json::str(model.name.clone())),
            ("fingerprint", Json::str(model.fingerprint.clone())),
            (
                "sources",
                Json::Object(
                    model
                        .sources
                        .iter()
                        .map(|(path, hash)| (path.clone(), Json::str(hash.clone())))
                        .collect(),
                ),
            ),
            (
                "component",
                model
                    .component
                    .as_ref()
                    .map_or(Json::Null, |name| Json::str(name.clone())),
            ),
            ("cover", Json::str(self.cover.name())),
            ("max_steps", Json::count(self.max_steps)),
            ("step_budget", Json::count(self.step_budget)),
            (
                "pair_limit",
                match self.cover {
                    Cover::Pairs { limit: Some(limit) } => Json::count(limit),
                    _ => Json::Null,
                },
            ),
            ("defaults", default_options()),
            (
                "fields",
                Json::Array(
                    model
                        .fields
                        .iter()
                        .map(|field| {
                            Json::object([
                                ("name", Json::str(field.name.clone())),
                                ("type", field_type_json(model, &field.kind)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "actions",
                Json::Array(
                    model
                        .actions
                        .iter()
                        .map(|action| {
                            Json::object([
                                ("name", Json::str(action.name.clone())),
                                (
                                    "params",
                                    Json::Array(
                                        action
                                            .params
                                            .iter()
                                            .map(|param| {
                                                Json::object([
                                                    ("name", Json::str(param.name.clone())),
                                                    ("type", domain_json(model, &param.domain)),
                                                ])
                                            })
                                            .collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("states", Json::Array(states)),
            ("parents", Json::Array(parents)),
            ("calls", Json::Array(calls)),
            ("tests", Json::Array(tests)),
            (
                "summary",
                Json::object([
                    ("states", Json::count(graph.state_count())),
                    ("transitions", Json::count(graph.transition_count())),
                    (
                        "refusals",
                        Json::count(
                            graph
                                .state_count()
                                .saturating_mul(model.calls.len())
                                .saturating_sub(graph.transition_count()),
                        ),
                    ),
                    (
                        "eligible_pairs",
                        match self.coverage {
                            Coverage::Pairs { eligible, .. } => {
                                Json::Int(i64::try_from(eligible).unwrap_or(i64::MAX))
                            }
                            Coverage::Obligations => Json::Null,
                        },
                    ),
                    (
                        "covered_pairs",
                        match self.coverage {
                            Coverage::Pairs { covered, .. } => Json::count(covered),
                            Coverage::Obligations => Json::Null,
                        },
                    ),
                    ("tests", Json::count(self.tests.len())),
                    ("accepted_steps", Json::count(self.accepted_steps)),
                    ("rejected_steps", Json::count(self.rejected_steps)),
                ]),
            ),
        ]);
        // Only plans that identify states carry the key, so plans built
        // without an observation are byte-identical to earlier ones.
        if let (Json::Object(entries), Some(identification)) = (&mut plan, &self.identification) {
            let fields = identification.fields.iter().map(Json::str).collect();
            entries.push(("observe".to_string(), Json::Array(fields)));
        }
        plan
    }
}

fn check_safe_integers(model: &Model) -> Result<()> {
    let unsafe_domain = |domain: &Domain| match domain {
        Domain::Integer { min, max } => *min < -MAX_SAFE_INTEGER || *max > MAX_SAFE_INTEGER,
        _ => false,
    };
    fn unsafe_kind(kind: &FieldType, unsafe_domain: &impl Fn(&Domain) -> bool) -> bool {
        match kind {
            FieldType::Scalar(domain) => unsafe_domain(domain),
            FieldType::Map { keys, value, .. } => {
                unsafe_domain(keys) || unsafe_kind(value, unsafe_domain)
            }
            FieldType::Record(members) => members
                .iter()
                .any(|(_, kind)| unsafe_kind(kind, unsafe_domain)),
        }
    }
    let fields = model
        .fields
        .iter()
        .any(|field| unsafe_kind(&field.kind, &unsafe_domain));
    let params = model
        .actions
        .iter()
        .flat_map(|action| &action.params)
        .any(|param| unsafe_domain(&param.domain));
    if fields || params {
        return Err(Error::new(format!(
            "test plans carry integers as JSON numbers, so every range must stay within ±{MAX_SAFE_INTEGER}"
        )));
    }
    Ok(())
}

pub(crate) fn value_json(value: &Value) -> Json {
    match value {
        Value::Integer(raw) => Json::Int(*raw),
        Value::Boolean(flag) => Json::Bool(*flag),
        Value::Symbol(name) => Json::str(name.clone()),
        Value::Map(entries) => Json::Object(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), value_json(value)))
                .collect(),
        ),
    }
}

pub(crate) fn state_json(state: &crate::State) -> Json {
    Json::Object(
        state
            .iter()
            .map(|(name, value)| (name.clone(), value_json(value)))
            .collect(),
    )
}

fn domain_json(model: &Model, domain: &Domain) -> Json {
    match domain {
        Domain::Integer { min, max } => Json::object([
            ("kind", Json::str("integer")),
            ("min", Json::Int(*min)),
            ("max", Json::Int(*max)),
        ]),
        Domain::Boolean => Json::object([("kind", Json::str("boolean"))]),
        Domain::Enum(ids) => Json::object([
            ("kind", Json::str("enum")),
            (
                "values",
                Json::Array(
                    ids.iter()
                        .map(|id| Json::str(model.symbol(i64::from(*id))))
                        .collect(),
                ),
            ),
        ]),
    }
}

fn field_type_json(model: &Model, kind: &FieldType) -> Json {
    match kind {
        FieldType::Scalar(domain) => domain_json(model, domain),
        FieldType::Map { keys, value, .. } => Json::object([
            ("kind", Json::str("map")),
            (
                "keys",
                Json::Array(
                    keys.raws()
                        .into_iter()
                        .map(|raw| Json::str(model.value(keys, raw).to_string()))
                        .collect(),
                ),
            ),
            ("value", field_type_json(model, value)),
        ]),
        FieldType::Record(members) => Json::object([
            ("kind", Json::str("record")),
            (
                "members",
                Json::Array(
                    members
                        .iter()
                        .map(|(name, kind)| {
                            Json::object([
                                ("name", Json::str(name.clone())),
                                ("type", field_type_json(model, kind)),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]),
    }
}

/// The options `zin plan` uses when none are given. A runner that picks a
/// cached plan by itself treats every option the caller left out as this
/// value, so a plan with weaker coverage is never chosen silently.
fn default_options() -> Json {
    let defaults = PlanOptions::default();
    Json::object([
        ("cover", Json::str(defaults.cover.name())),
        ("max_steps", Json::count(defaults.max_steps)),
        ("step_budget", Json::count(defaults.step_budget)),
        ("pair_limit", Json::Null),
    ])
}
