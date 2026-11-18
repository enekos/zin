use crate::check::Verification;
use crate::mermaid::{sequence_diagram, state_diagram};
use crate::plan::{Plan, PlanOptions};
use std::fmt::Write as _;

/// A Markdown report: what was proven, the derived test plan, and diagrams.
#[must_use]
pub fn render_markdown(verification: &Verification) -> String {
    let model = &verification.model;
    let graph = &verification.graph;
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", model.name);
    let _ = writeln!(
        out,
        "Every reachable state was explored: **{} states**, **{} transitions**, {} terminal.\n",
        graph.state_count(),
        graph.transition_count(),
        verification.terminal_states
    );

    if !verification.properties.is_empty() {
        let _ = writeln!(out, "## Proven properties\n");
        for property in &verification.properties {
            let _ = writeln!(out, "- `{}`", property.text);
        }
        out.push('\n');
    }
    if let Some(terminal) = &model.terminal {
        let _ = writeln!(
            out,
            "No deadlock: the system only stops when `{}`.\n",
            terminal.source
        );
    }
    if !verification.notes.is_empty() {
        let _ = writeln!(out, "## Notes\n");
        for note in &verification.notes {
            let _ = writeln!(out, "- {note}");
        }
        out.push('\n');
    }
    if let Ok(plan) = Plan::build(verification, &PlanOptions::default()) {
        let _ = writeln!(out, "## Conformance plan\n");
        let _ = writeln!(
            out,
            "`zin plan` derives {} that together exercise every transition and every refusal.\n",
            plan.summary()
        );
    }
    let _ = writeln!(
        out,
        "## States\n\n```mermaid\n{}```\n",
        state_diagram(verification)
    );
    for scenario in &verification.scenarios {
        let _ = writeln!(
            out,
            "## Scenario: {}\n\n```mermaid\n{}```\n",
            scenario.name,
            sequence_diagram(verification, scenario)
        );
    }
    out
}
