use crate::ast::{Fairness, PropertyKind, Spec};
use crate::explore::{Graph, explore};
use crate::model::{Absence, CompiledStep, Model, index32};
use crate::properties::check_properties;
use crate::symmetry::Symmetry;
use crate::{Error, ErrorKind, Result, Trace, TraceStep};
use std::sync::Arc;

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

/// How to check a specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// Explore one representative per class of states that differ only by a
    /// relabeling of interchangeable values. The graph is then not the full
    /// state space, so conformance plans need it off.
    pub symmetry: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { symmetry: true }
    }
}

/// A read that faults when a value is absent and that no condition before
/// it guards. No reachable state makes it fault, or the check would have
/// failed, but nothing in the specification rules it out either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnguardedRead {
    /// The expression that reads, such as `left of lease`.
    pub read: String,
    /// What can be absent, such as `lease`.
    pub subject: String,
    /// The name that starts the read.
    pub span: Option<crate::SourceSpan>,
    /// The note `zin check` prints, without its location.
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct Verification {
    pub model: Model,
    /// The explored states; representatives only when [`Self::reduced`].
    pub graph: Graph,
    /// Whether symmetry reduction shrank the graph.
    pub reduced: bool,
    pub properties: Vec<PropertyResult>,
    pub scenarios: Vec<ScenarioRun>,
    pub notes: Vec<String>,
    /// Reads that fault when a value is absent and that nothing guards; each
    /// is also in [`Self::notes`].
    pub unguarded: Vec<UnguardedRead>,
    pub terminal_states: usize,
}

impl Verification {
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{}: {}{}, {}, {}, {}",
            self.model.name,
            crate::counted(self.graph.state_count(), "state", "states"),
            if self.reduced { " up to symmetry" } else { "" },
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

/// Checks a specification over its full state space, as conformance plans,
/// trace checking and diagrams need.
///
/// # Errors
///
/// Returns the first problem found: an invalid specification, a model fault,
/// a violated property or scenario (with a counterexample), or a limit.
pub fn verify(spec: &Spec) -> Result<Verification> {
    verify_with(spec, Options { symmetry: false })
}

/// Checks a specification, with symmetry reduction when `options` allow it.
///
/// # Errors
///
/// As [`verify`].
pub fn verify_with(spec: &Spec, options: Options) -> Result<Verification> {
    verify_unlocated(spec, options).map_err(|e| spec.locate(e))
}

/// A counterexample found on symmetry representatives that does not replay
/// as a real run would be reported wrongly, so the check is repeated on the
/// full state space instead.
fn verify_unlocated(spec: &Spec, options: Options) -> Result<Verification> {
    match verify_once(spec, options) {
        Err(error) if error.unlifted() => {
            let mut verification = verify_once(spec, Options { symmetry: false })?;
            verification.notes.insert(
                0,
                "symmetry reduction is off: a counterexample on representatives did not replay as a real run, so the full state space was checked"
                    .to_string(),
            );
            Ok(verification)
        }
        result => result,
    }
}

fn verify_once(spec: &Spec, options: Options) -> Result<Verification> {
    let model = Model::compile_unlocated(spec)?;
    let mut notes = Vec::new();
    let symmetry = if options.symmetry && !model.interchangeable.is_empty() {
        reduction(&model, &mut notes)
    } else {
        None
    };
    let graph = explore(&model, symmetry)?;
    let terminal_states = check_terminal(&model, &graph)?;
    let checked = check_properties(&model, &graph)?;
    notes.extend(checked.notes);
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
    let unguarded = unguarded_reads(spec, &model);
    notes.extend(unguarded.iter().map(|read| match &read.span {
        Some(span) if !span.file.is_empty() => {
            format!(
                "{}:{}:{}: {}",
                span.file, span.line, span.column, read.message
            )
        }
        Some(span) => format!("line {}: {}", span.line, read.message),
        None => read.message.clone(),
    }));
    Ok(Verification {
        model,
        reduced: graph.reduced(),
        graph,
        properties: checked.results,
        scenarios,
        notes,
        unguarded,
        terminal_states,
    })
}

fn unguarded_reads(spec: &Spec, model: &Model) -> Vec<UnguardedRead> {
    crate::presence::unguarded(spec, model)
        .into_iter()
        .map(|read| {
            let fault = match &read.absence {
                Absence::Nothing => format!("{} is nothing", read.subject),
                Absence::Empty => format!("{} is empty", read.subject),
                Absence::Case { others, .. } => {
                    format!("{} is {}", read.subject, others.join(" or "))
                }
            };
            let guard = match &read.absence {
                Absence::Nothing => format!("{} is not nothing", read.subject),
                Absence::Empty => format!("{} is not []", read.subject),
                Absence::Case { holders, .. } => format!(
                    "{} is {}",
                    read.subject,
                    holders.first().map_or("?", String::as_str)
                ),
            };
            UnguardedRead {
                message: format!(
                    "'{}' faults if {fault}; no reachable state gets there, but nothing guards it: write '{guard} and ...' before it, or '{} or else ...'",
                    read.read, read.read
                ),
                span: spec.source_span(read.span),
                read: read.read,
                subject: read.subject,
            }
        })
        .collect()
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
                    .with_counterexample(graph.trace_to(model, id))
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
            .with_counterexample(graph.trace_to(model, id)));
        }
    }
    Ok(count)
}

/// Whether a liveness property depends on fairness. Fairness is per call,
/// and relabeling changes which call a representative's edge stands for, so
/// these are not checked on representatives.
/// The relabelings to explore with, or `None` with the reason in `notes`.
fn reduction(model: &Model, notes: &mut Vec<String>) -> Option<Arc<Symmetry>> {
    if fair_liveness(model) {
        notes.push(
            "symmetry reduction is off: liveness under fairness is checked on the full state space"
                .to_string(),
        );
        return None;
    }
    if crate::definedness::order_sensitive(model) {
        notes.push(
            "symmetry reduction is off: a short-circuiting quantifier over interchangeable values can fault for one value and stop early for another"
                .to_string(),
        );
        return None;
    }
    match Symmetry::new(model, &model.interchangeable) {
        Ok(symmetry) => Some(Arc::new(symmetry)),
        Err(error) => {
            notes.push(format!("symmetry reduction is off: {}", error.message));
            None
        }
    }
}

fn fair_liveness(model: &Model) -> bool {
    let fair = model
        .actions
        .iter()
        .any(|action| action.fairness != Fairness::Unfair);
    let liveness = model.properties.iter().any(|property| {
        matches!(
            property.kind,
            PropertyKind::Eventually | PropertyKind::LeadsTo
        )
    });
    fair && liveness
}

fn run_scenarios(model: &Model, graph: &Graph) -> Result<Vec<ScenarioRun>> {
    let mut locals = model.new_locals();
    let mut runs = Vec::new();
    for scenario in &model.scenarios {
        let mut state: Box<[i64]> = model.initial.clone();
        let mut trace = Trace {
            initial: model.decode(&state),
            steps: Vec::new(),
            loop_from: None,
        };
        let mut events = Vec::new();
        let fail = |line: usize, message: String, trace: &Trace| {
            Error::of(
                ErrorKind::Violation,
                Some(line),
                format!("scenario \"{}\": {message}", scenario.name),
            )
            .with_trace(trace.clone())
        };
        let step = |call: u32, state: &[i64], locals: &mut [i64], trace: &Trace, line: usize| {
            model.step(call, state, locals).map_err(|fault| {
                let message = format!("{}: {}", model.call_text(call), fault.message);
                fail(fault.line.max(line), message, trace)
            })
        };
        for scenario_step in &scenario.steps {
            match scenario_step {
                CompiledStep::Call { call, line } => {
                    let Some(next) = step(*call, &state, &mut locals, &trace, *line)? else {
                        return Err(fail(
                            *line,
                            format!(
                                "{} is not allowed here; a 'requires' rejects it",
                                model.call_text(*call)
                            ),
                            &trace,
                        ));
                    };
                    let representative = graph.canonical(next.clone());
                    let to = graph.index.get(&representative).copied().ok_or_else(|| {
                        fail(
                            *line,
                            "internal error: unexplored state".to_string(),
                            &trace,
                        )
                    })?;
                    state = next;
                    trace.steps.push(TraceStep {
                        call: model.call_text(*call),
                        state: model.decode(&state),
                    });
                    events.push(ScenarioEvent::Call { call: *call, to });
                }
                CompiledStep::Reject { call, line } => {
                    if step(*call, &state, &mut locals, &trace, *line)?.is_some() {
                        return Err(fail(
                            *line,
                            format!(
                                "expected {} to be rejected, but the specification allows it",
                                model.call_text(*call)
                            ),
                            &trace,
                        ));
                    }
                    events.push(ScenarioEvent::Reject { call: *call });
                }
                CompiledStep::Expect(condition) => {
                    let holds = model
                        .holds(condition, &state, &mut locals)
                        .map_err(|fault| fail(fault.line, fault.message, &trace))?;
                    if !holds {
                        return Err(fail(
                            condition.line,
                            format!("expected {}", condition.source),
                            &trace,
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
