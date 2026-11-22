use zin::conform::{Adapter, Args, Outcome, check};
use zin::{ErrorKind, PlanOptions, State, Value, audit, check_trace_reader, parse, verify};

const SWITCH: &str = r#"system "Switch"
state
  active is a boolean
initially
  active is false
action start
  requires not active
  active becomes true
action stop
  requires active
  active becomes false
"#;

#[test]
fn trace_streaming_checks_exact_byte_limit_and_line_numbers() {
    let model = verify(&parse(SWITCH).unwrap()).unwrap().model;
    let good = b"{\"action\":\"start\"}\n";
    let read = |bytes: &[u8], limit| {
        check_trace_reader(&model, std::io::BufReader::with_capacity(2, bytes), limit)
    };
    assert_eq!(read(good, good.len() as u64).unwrap().events, 1);
    assert_eq!(
        read(good, good.len() as u64 - 1).unwrap_err().kind,
        ErrorKind::Usage
    );
    let err = read(b"\n{\"action\":\"stop\"}\n", 200).unwrap_err();
    assert!(err.message.contains("line 2"), "{err}");
}

#[test]
fn audit_gives_a_shortest_distinguishing_example_for_every_survivor() {
    let report = audit(&parse(SWITCH).unwrap()).unwrap();
    let survivors = report.survivors();
    assert!(!survivors.is_empty());
    for mutant in survivors {
        let witness = mutant.witness.as_ref().unwrap();
        assert_ne!(witness.original, witness.mutated);
        assert!(witness.prefix.len() <= 1);
    }
    let dropped = report
        .mutants
        .iter()
        .find(|m| m.description == "action start: drop 'requires not active'")
        .unwrap();
    let witness = dropped.witness.as_ref().unwrap();
    assert_eq!(witness.prefix, ["start"]);
    assert_eq!(witness.call, "start");
    assert!(witness.original.is_none() && witness.mutated.is_some());
}

/// A switch whose observation lies once `lies` says so, to control whether a
/// shorter path reaches the same failure.
struct Switch {
    active: bool,
    accepted: usize,
    rejected: usize,
    lies: fn(&Switch) -> bool,
}

impl Switch {
    fn lying(lies: fn(&Switch) -> bool) -> impl FnMut() -> Self {
        move || Self {
            active: false,
            accepted: 0,
            rejected: 0,
            lies,
        }
    }
}

impl Adapter for Switch {
    fn call(&mut self, action: &str, _: &Args) -> Outcome {
        if (action == "start") == self.active {
            self.rejected += 1;
            return Outcome::Rejected;
        }
        self.accepted += 1;
        self.active = !self.active;
        Outcome::Accepted
    }

    fn observe(&self) -> Option<State> {
        let active = self.active != (self.lies)(self);
        Some(State::from([("active".into(), Value::Boolean(active))]))
    }
}

#[test]
fn a_report_carries_a_pasteable_scenario_and_a_verified_shorter_reproduction() {
    let error = check(
        SWITCH,
        &PlanOptions::default(),
        Switch::lying(|s| s.accepted >= 2),
    )
    .unwrap_err();
    let fingerprint = parse(SWITCH).unwrap().fingerprint;
    let message = &error.message;
    assert!(
        message.contains(&format!("spec fingerprint: {fingerprint}")),
        "{message}"
    );
    assert!(
        message.contains("scenario \"Replay divergence\"\n"),
        "{message}"
    );
    assert!(message.contains("\n  stop"), "{message}");
    let start = message.find("scenario \"Replay divergence\"").unwrap();
    let scenario: Vec<_> = message[start..]
        .lines()
        .take_while(|l| !l.starts_with("shortest") && !l.is_empty())
        .collect();
    let pasted = format!("{SWITCH}{}\n", scenario.join("\n"));
    assert_eq!(verify(&parse(&pasted).unwrap()).unwrap().scenarios.len(), 1);
    let (_, short) = message
        .split_once("shortest reproduction:\n")
        .expect(message);
    assert_eq!(
        short
            .lines()
            .filter(|l| l.contains("start") || l.contains("stop"))
            .count(),
        2,
        "{message}"
    );
}

#[test]
fn a_history_dependent_failure_keeps_the_original_reproduction() {
    let error = check(
        SWITCH,
        &PlanOptions::default(),
        Switch::lying(|s| s.rejected >= 1 && s.accepted >= 2),
    )
    .unwrap_err();
    assert!(!error.message.contains("shortest reproduction"), "{error}");
    assert!(
        error
            .message
            .contains("does not reproduce the original mismatch"),
        "{error}"
    );
}
