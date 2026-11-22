use crate::check::{ScenarioEvent, ScenarioRun, Verification};
use crate::display_state;
use crate::model::index32;
use std::fmt::Write as _;

pub const MAX_DIAGRAM_STATES: usize = 60;

fn label(text: &str) -> String {
    text.replace('"', "'").replace(['\n', '\r'], " ")
}

/// A Mermaid state diagram of the reachable states, in discovery order.
/// Large graphs are cut off after [`MAX_DIAGRAM_STATES`] states.
#[must_use]
pub fn state_diagram(verification: &Verification) -> String {
    let graph = &verification.graph;
    let shown = graph.state_count().min(MAX_DIAGRAM_STATES);
    let mut out = String::from("stateDiagram-v2\n");
    if graph.reduced {
        out.push_str("    note right of s0: states are representatives up to symmetry\n");
    }
    for state in 0..shown {
        let _ = writeln!(
            out,
            "    state \"{}\" as s{state}",
            label(&display_state(&verification.state(index32(state))))
        );
    }
    out.push_str("    [*] --> s0\n");
    for state in 0..shown {
        let id = index32(state);
        let out_edges = graph.out(id);
        if out_edges.is_empty() {
            let _ = writeln!(out, "    s{state} --> [*]");
        }
        let mut targets: Vec<u32> = out_edges.iter().map(|edge| edge.to).collect();
        targets.sort_unstable();
        targets.dedup();
        for to in targets {
            if crate::model::at(to) >= shown {
                continue;
            }
            let calls = out_edges
                .iter()
                .filter(|edge| edge.to == to)
                .map(|edge| verification.call_text(edge.call))
                .collect::<Vec<_>>()
                .join(" / ");
            let _ = writeln!(out, "    s{state} --> s{to}: {}", label(&calls));
        }
    }
    if shown < graph.state_count() {
        let _ = writeln!(
            out,
            "    note right of s0: showing {shown} of {} states",
            graph.state_count()
        );
    }
    out
}

/// A Mermaid sequence diagram of one scenario run.
#[must_use]
pub fn sequence_diagram(verification: &Verification, scenario: &ScenarioRun) -> String {
    let mut out = String::from("sequenceDiagram\n");
    let _ = writeln!(out, "    participant Client");
    let _ = writeln!(
        out,
        "    participant System as {}",
        label(&verification.model.name)
    );
    let _ = writeln!(
        out,
        "    Note over System: {}",
        label(&display_state(&verification.state(0)))
    );
    let mut concrete = verification.model.initial.clone();
    let mut locals = verification.model.new_locals();
    for event in &scenario.events {
        match event {
            ScenarioEvent::Call { call, to } => {
                let observed = if verification.graph.reduced {
                    match verification.model.step(*call, &concrete, &mut locals) {
                        Ok(Some(next)) => {
                            concrete = next;
                            verification.model.decode(&concrete)
                        }
                        _ => {
                            out.push_str("    Note right of System: scenario cannot be replayed\n");
                            break;
                        }
                    }
                } else {
                    verification.state(*to)
                };
                let _ = writeln!(
                    out,
                    "    Client->>System: {}",
                    label(&verification.call_text(*call))
                );
                let _ = writeln!(
                    out,
                    "    Note over System: {}",
                    label(&display_state(&observed))
                );
            }
            ScenarioEvent::Reject { call } => {
                let _ = writeln!(
                    out,
                    "    Client-xSystem: {} (rejected)",
                    label(&verification.call_text(*call))
                );
            }
            ScenarioEvent::Expect { source } => {
                let _ = writeln!(out, "    Note right of System: expect {}", label(source));
            }
        }
    }
    out
}
