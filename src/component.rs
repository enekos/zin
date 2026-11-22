//! Isolate one component while keeping the composition's worked examples.
use crate::ast::{ActionCall, CallArguments};
use crate::model::{named_values, resolve_call};
use crate::{Error, Result, ScenarioEvent, ScenarioRun, Spec, Verification, verify};

/// Check the complete composition, then explore the selected component with a
/// controllable environment. Shared calls can be offered whenever the local
/// guard allows them. Whole-system scenarios seed the local test plan.
pub fn verify_component(spec: &Spec, name: &str) -> Result<Verification> {
    let component = spec
        .component_specs
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| {
            Error::usage(format!(
                "unknown component '{name}'; available: {}",
                spec.components.join(", ")
            ))
        })?;
    if component.state.is_empty() {
        return Err(Error::usage(format!(
            "component '{name}' has no state to test"
        )));
    }
    let whole = verify(spec)?;
    let mut local = component.clone();
    local.types = spec.types.clone();
    local.state_limit = spec.state_limit;
    local.fingerprint = spec.fingerprint.clone();
    local.sources = spec.sources.clone();
    local.source_map = spec.source_map.clone();
    // Properties, terminal conditions and scenarios have whole-system scope.
    local.properties.clear();
    local.scenarios.clear();
    local.terminal = None;
    let mut isolated = verify(&local)?;
    isolated.model.component = Some(name.into());
    let mut calls = vec![None; whole.model.call_count()];
    for (i, local_id) in calls.iter_mut().enumerate() {
        let call = crate::model::index32(i);
        let Some(action) = isolated
            .model
            .actions
            .iter()
            .find(|a| a.name == whole.model.call_action(call))
        else {
            continue;
        };
        let mut args = named_values(&whole.model, call);
        args.retain(|name, _| action.params.iter().any(|p| p.name == *name));
        *local_id = Some(resolve_call(
            &isolated.model,
            &ActionCall {
                name: action.name.clone(),
                line: action.line,
                arguments: CallArguments::Named(args),
            },
        )?);
    }
    for scenario in &whole.scenarios {
        let mut state = 0;
        let mut events = Vec::new();
        for event in &scenario.events {
            let call = match event {
                ScenarioEvent::Call { call, .. } | ScenarioEvent::Reject { call } => *call,
                ScenarioEvent::Expect { .. } => continue,
            };
            let Some(call) = calls[crate::model::at(call)] else {
                continue;
            };
            let target = isolated.graph.target(state, call);
            match (event, target) {
                (ScenarioEvent::Call { .. }, Some(to)) => {
                    events.push(ScenarioEvent::Call { call, to });
                    state = to;
                }
                (ScenarioEvent::Call { .. }, None) => {
                    return Err(Error::new(
                        "composition and component disagree on a shared action",
                    ));
                }
                (ScenarioEvent::Reject { .. }, None) => events.push(ScenarioEvent::Reject { call }),
                // The environment alone blocked this call. It never reaches the component.
                _ => {}
            }
        }
        if !events.is_empty() {
            isolated.scenarios.push(ScenarioRun {
                name: scenario.name.clone(),
                line: scenario.line,
                events,
            });
        }
    }
    Ok(isolated)
}
