use crate::check::{ScenarioResult, State, Verification};

const MAX_DIAGRAM_STATES: usize = 40;
const MAX_DIAGRAM_TRANSITIONS: usize = 120;

#[must_use]
pub fn state_diagram(verification: &Verification) -> String {
    let state_count = verification.states.len().min(MAX_DIAGRAM_STATES);
    let mut output = String::from("stateDiagram-v2\n  direction LR\n");
    output.push_str("  [*] --> s0\n");
    for (index, state) in verification.states.iter().take(state_count).enumerate() {
        output.push_str(&format!("  state \"{}\" as s{index}\n", state_label(state)));
    }
    let mut shown = 0;
    for transition in &verification.transitions {
        if transition.from >= state_count || transition.to >= state_count {
            continue;
        }
        if shown == MAX_DIAGRAM_TRANSITIONS {
            break;
        }
        output.push_str(&format!(
            "  s{} --> s{} : {}\n",
            transition.from,
            transition.to,
            escape(&transition.call.display())
        ));
        shown += 1;
    }
    if state_count < verification.states.len() || shown < verification.transitions.len() {
        output.push_str(&format!(
            "  %% Diagram limited to {state_count} states and {shown} transitions; verification covered {} states and {} transitions.\n",
            verification.states.len(),
            verification.transitions.len()
        ));
    }
    output
}

#[must_use]
pub fn sequence_diagram(system_name: &str, scenario: &ScenarioResult) -> String {
    let mut output =
        String::from("sequenceDiagram\n  autonumber\n  participant reviewer as Reviewer\n");
    output.push_str(&format!(
        "  participant system as {}\n",
        escape(system_name)
    ));
    if let Some(initial) = scenario.states.first() {
        output.push_str(&format!(
            "  Note over system: Initially<br/>{}\n",
            state_label(initial)
        ));
    }
    for (call, state) in scenario.calls.iter().zip(scenario.states.iter().skip(1)) {
        output.push_str(&format!(
            "  reviewer->>system: {}\n",
            escape(&call.display())
        ));
        output.push_str(&format!("  Note over system: {}\n", state_label(state)));
    }
    output
}

fn state_label(state: &State) -> String {
    state
        .iter()
        .map(|(name, value)| format!("{} = {}", escape(name), escape(&value.source())))
        .collect::<Vec<_>>()
        .join("<br/>")
}

fn escape(source: &str) -> String {
    source
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace(['\n', '\r'], " ")
}
