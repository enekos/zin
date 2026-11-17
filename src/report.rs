use crate::ast::Spec;
use crate::check::{Verification, display_state};
use crate::mermaid::{sequence_diagram, state_diagram};

#[must_use]
pub fn render_markdown(spec: &Spec, verification: &Verification) -> String {
    let mut output = format!(
        "# {}\n\n> A Zin behavioral review. **All {} reachable states satisfy {} promises.**\n\n",
        one_line(&spec.name),
        verification.states.len(),
        verification.promises
    );
    output.push_str(&format!(
        "**{}** states · **{}** transitions · **{}** actions · **{}** scenarios · **{}** terminal states\n\n",
        verification.states.len(),
        verification.transitions.len(),
        spec.actions.len(),
        verification.scenarios.len(),
        verification.terminal_states()
    ));

    output.push_str("## State\n\n| Field | Domain | Initially |\n|---|---|---|\n");
    for field in &spec.state {
        let initial = verification
            .initial
            .get(&field.name)
            .map_or_else(|| "—".to_string(), |value| value.source());
        output.push_str(&format!(
            "| `{}` | {} | `{}` |\n",
            field.name, field.kind, initial
        ));
    }

    output.push_str("\n## Promises\n\n");
    if spec.invariants.is_empty() {
        output.push_str("No safety promises declared.\n");
    } else {
        for (index, promise) in spec.invariants.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", index + 1, one_line(&promise.source)));
        }
    }

    let coverage = verification.action_coverage();
    output.push_str("\n## Actions\n\n");
    for action in &spec.actions {
        output.push_str(&format!("### {}\n\n", one_line(&action.name)));
        if !action.params.is_empty() {
            output.push_str("| Parameter | Domain |\n|---|---|\n");
            for parameter in &action.params {
                output.push_str(&format!("| `{}` | {} |\n", parameter.name, parameter.kind));
            }
            output.push('\n');
        }
        output.push_str("**Allowed when**\n\n");
        if action.requires.is_empty() {
            output.push_str("- Always\n");
        } else {
            for condition in &action.requires {
                output.push_str(&format!("- {}\n", one_line(&condition.source)));
            }
        }
        output.push_str("\n**Changes**\n\n");
        if action.assignments.is_empty() {
            output.push_str("- No state changes\n");
        } else {
            for assignment in &action.assignments {
                output.push_str(&format!("- {}\n", one_line(&assignment.source)));
            }
        }
        let count = coverage.get(&action.name).copied().unwrap_or(0);
        if count == 0 {
            output.push_str(
                "\n**Review note:** this action is never enabled in the reachable model.\n",
            );
        }
        output.push('\n');
    }

    output.push_str("## Reachable behavior\n\nEvery node below is a complete system state. Every edge is an enabled action.\n\n```mermaid\n");
    output.push_str(&state_diagram(verification));
    output.push_str("```\n");

    if !spec.scenarios.is_empty() {
        output.push_str("\n## Journeys\n\n");
        for scenario in &verification.scenarios {
            output.push_str(&format!("### {}\n\n", one_line(&scenario.name)));
            output.push_str(&format!(
                "{} calls · {} expectations passed\n\n```mermaid\n",
                scenario.calls.len(),
                scenario.expectations
            ));
            output.push_str(&sequence_diagram(&spec.name, scenario));
            output.push_str("```\n\n");
            if let Some(final_state) = scenario.states.last() {
                output.push_str(&format!(
                    "**Final state:** {}\n\n",
                    display_state(final_state)
                ));
            }
        }
    }

    output.push_str("---\nVerification is exhaustive within the declared finite domains. Assignments in one action read the pre-action state and commit simultaneously.\n");
    output
}

fn one_line(source: &str) -> String {
    source.replace(['\n', '\r'], " ")
}
