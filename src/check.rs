use crate::ast::{PropertyKind, Spec};
use crate::explore::{Edge, Graph, explore};
use crate::model::{CompiledStep, Model, index32};
use crate::properties::check_properties;
use crate::{Error, ErrorKind, Result};

#[derive(Debug, Clone)]
pub struct PropertyResult {
    pub text: String,
    pub kind: PropertyKind,
    pub line: usize,
    pub witness: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioEvent {
    Call { call: u32, to: u32 },
    Reject { call: u32 },
    Expect { source: String },
}

#[derive(Debug, Clone)]
pub struct ScenarioRun {
    pub name: String,
    pub line: usize,
    pub events: Vec<ScenarioEvent>,
}

#[derive(Debug, Clone)]
pub struct Verification {
    pub model: Model,
    pub graph: Graph,
    pub properties: Vec<PropertyResult>,
    pub scenarios: Vec<ScenarioRun>,
    pub notes: Vec<String>,
    pub terminal_states: usize,
}

impl Verification {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{}: {}, {}, {}, {}",
            self.model.name,
            crate::counted(self.graph.state_count(), "state", "states"),
            crate::counted(self.graph.transition_count(), "transition", "transitions"),
            crate::counted(self.properties.len(), "property", "properties"),
            crate::counted(self.scenarios.len(), "scenario", "scenarios")
        )
    }

    #[must_use]
    pub fn call_text(&self, call: u32) -> String {
        self.model.call_text(call)
    }

    #[must_use]
    pub fn state(&self, state: u32) -> crate::State {
        self.model.decode(self.graph.state(state))
    }
}

pub fn verify(spec: &Spec) -> Result<Verification> {
    let model = Model::compile(spec)?;
    let graph = explore(&model)?;
    let terminal_states = check_terminal(&model, &graph)?;
    let checked = check_properties(&model, &graph)?;
    let mut notes = checked.notes;
    let mut used = vec![false; model.actions.len()];
    for edge in &graph.edges {
        let action = model.calls.get(crate::model::at(edge.call));
        if let Some(seen) = action.and_then(|call| used.get_mut(call.action)) {
            *seen = true;
        }
    }
    for (action, used) in model.actions.iter().zip(used) {
        if !used {
            notes.push(format!(
                "action {} is never enabled in any reachable state",
                action.name
            ));
        }
    }
    let scenarios = run_scenarios(&model, &graph)?;
    Ok(Verification {
        model,
        graph,
        properties: checked.results,
        scenarios,
        notes,
        terminal_states,
    })
}

fn check_terminal(model: &Model, graph: &Graph) -> Result<usize> {
    let mut locals = model.new_locals();
    let mut count = 0;
    for state in 0..graph.state_count() {
        let id = index32(state);
        if !graph.out(id).is_empty() {
            continue;
        }
        count += 1;
        let Some(terminal) = &model.terminal else {
            continue;
        };
        let allowed = model
            .holds(terminal, graph.state(id), &mut locals)
            .map_err(|fault| {
                Error::of(ErrorKind::Model, Some(fault.line), fault.message)
                    .with_trace(graph.trace_to(model, id))
            })?;
        if !allowed {
            return Err(Error::of(
                ErrorKind::Violation,
                Some(terminal.line),
                format!(
                    "deadlock: no action is enabled, but 'terminal when {}' does not hold",
                    terminal.source
                ),
            )
            .with_trace(graph.trace_to(model, id)));
        }
    }
    Ok(count)
}

fn run_scenarios(model: &Model, graph: &Graph) -> Result<Vec<ScenarioRun>> {
    let mut locals = model.new_locals();
    let mut runs = Vec::new();
    for scenario in &model.scenarios {
        let mut current: u32 = 0;
        let mut path: Vec<Edge> = Vec::new();
        let mut events = Vec::new();
        let fail = |line: usize, message: String, path: &[Edge]| {
            Error::of(
                ErrorKind::Violation,
                Some(line),
                format!("scenario \"{}\": {message}", scenario.name),
            )
            .with_trace(graph.trace(model, path, None))
        };
        for step in &scenario.steps {
            match step {
                CompiledStep::Call { call, line } => {
                    let Some(to) = graph.target(current, *call) else {
                        return Err(fail(
                            *line,
                            format!(
                                "{} is not allowed here; a 'requires' rejects it",
                                model.call_text(*call)
                            ),
                            &path,
                        ));
                    };
                    path.push(Edge { call: *call, to });
                    events.push(ScenarioEvent::Call { call: *call, to });
                    current = to;
                }
                CompiledStep::Reject { call, line } => {
                    if graph.target(current, *call).is_some() {
                        return Err(fail(
                            *line,
                            format!(
                                "expected {} to be rejected, but the specification allows it",
                                model.call_text(*call)
                            ),
                            &path,
                        ));
                    }
                    events.push(ScenarioEvent::Reject { call: *call });
                }
                CompiledStep::Expect(condition) => {
                    let holds = model
                        .holds(condition, graph.state(current), &mut locals)
                        .map_err(|fault| fail(fault.line, fault.message, &path))?;
                    if !holds {
                        return Err(fail(
                            condition.line,
                            format!("expected {}", condition.source),
                            &path,
                        ));
                    }
                    events.push(ScenarioEvent::Expect {
                        source: condition.source.clone(),
                    });
                }
            }
        }
        runs.push(ScenarioRun {
            name: scenario.name.clone(),
            line: scenario.line,
            events,
        });
    }
    Ok(runs)
}
