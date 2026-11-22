use zin::{ErrorKind, Options, Trace, Verification, parse, verify, verify_with};

fn reduced(source: &str) -> zin::Result<Verification> {
    verify_with(&parse(source)?, Options::default())
}

fn full(source: &str) -> zin::Result<Verification> {
    verify(&parse(source)?)
}

/// Every step of `trace` is a transition of the full state space, and a loop
/// returns to the state it started from.
fn assert_replays(full: &Verification, trace: &Trace) {
    let mut current = 0;
    assert_eq!(full.state(current), trace.initial, "initial state");
    let mut visited = vec![current];
    for (index, step) in trace.steps.iter().enumerate() {
        let edge = full
            .graph
            .out(current)
            .iter()
            .find(|edge| full.call_text(edge.call) == step.call)
            .unwrap_or_else(|| panic!("step {}: {} is not enabled\n{trace}", index + 1, step.call));
        current = edge.to;
        assert_eq!(
            full.state(current),
            step.state,
            "step {}\n{trace}",
            index + 1
        );
        visited.push(current);
    }
    if let Some(loop_from) = trace.loop_from {
        assert_eq!(
            visited.last(),
            visited.get(loop_from),
            "the loop does not close\n{trace}"
        );
    }
}

/// Reduced and full checking agree, and a reduced counterexample is a real run.
fn assert_agree(source: &str) -> Option<zin::Error> {
    let concrete = full(source);
    let symmetric = reduced(source);
    match (&concrete, &symmetric) {
        (Ok(concrete), Ok(symmetric)) => {
            assert!(symmetric.reduced);
            assert!(symmetric.graph.state_count() <= concrete.graph.state_count());
            assert_eq!(symmetric.properties.len(), concrete.properties.len());
            None
        }
        (Err(expected), Err(actual)) => {
            assert_eq!(actual.kind, expected.kind, "{actual}");
            assert_eq!(actual.line, expected.line, "{actual}");
            assert_eq!(actual.message, expected.message);
            let mut bare = parse(source).unwrap();
            bare.properties.clear();
            bare.scenarios.clear();
            bare.terminal = None;
            let concrete = verify(&bare).expect("the state space without properties");
            if let Some(trace) = &actual.trace {
                let concrete = &concrete;
                assert_replays(concrete, trace);
                let shortest = expected.trace.as_ref().expect("a full counterexample");
                if trace.loop_from.is_none() {
                    assert_eq!(trace.steps.len(), shortest.steps.len(), "{trace}");
                }
            }
            Some(actual.clone())
        }
        (expected, actual) => panic!("full: {expected:?}\nreduced: {actual:?}"),
    }
}

const MUTEX: &str = include_str!("../examples/mutex.zin");

#[test]
fn interchangeable_workers_shrink_the_state_space() {
    let symmetric = reduced(MUTEX).unwrap();
    let concrete = full(MUTEX).unwrap();
    assert_eq!(symmetric.graph.state_count(), 9);
    assert_eq!(concrete.graph.state_count(), 48);
    assert!(symmetric.summary().contains("9 states up to symmetry"));
    assert!(!concrete.reduced);
    assert_agree(MUTEX);
}

#[test]
fn a_broken_lock_has_the_same_verdict_and_a_real_counterexample() {
    let broken = MUTEX.replace(
        "requires at of w is waiting and holder is nobody",
        "requires at of w is waiting",
    );
    let error = assert_agree(&broken).expect("a violation");
    assert_eq!(error.kind, ErrorKind::Violation);
    let trace = error.trace.expect("a counterexample");
    let Some(zin::Value::Map(at)) = trace.steps.last().and_then(|step| step.state.get("at")) else {
        panic!("{trace}");
    };
    let critical = at
        .values()
        .filter(|phase| phase.to_string() == "critical")
        .count();
    assert_eq!(critical, 2, "{trace}");
}

#[test]
fn lifted_safety_traces_relabel_calls_along_the_way() {
    let source = r#"system "Climbers"
types
  interchangeable climber is one of ann, ben, cid
state
  level is a map from climber to an integer from 0 to 3
initially
  level of every climber is 0
never
  some a in climber satisfies some b in climber satisfies level of a is 3 and level of b is 2
action climb(c is a climber)
  requires level of c < 3
  level of c becomes level of c + 1
action fall(c is a climber)
  requires level of c > 0
  level of c becomes 0
"#;
    let error = assert_agree(source).expect("a violation");
    let trace = error.trace.expect("a counterexample");
    assert_eq!(trace.steps.len(), 5, "{trace}");
    let Some(zin::Value::Map(levels)) = trace.steps.last().and_then(|step| step.state.get("level"))
    else {
        panic!("{trace}");
    };
    let mut levels: Vec<String> = levels.values().map(ToString::to_string).collect();
    levels.sort();
    assert_eq!(levels, ["0", "2", "3"], "{trace}");
}

#[test]
fn lifted_loops_close_on_a_concrete_state() {
    let source = r#"system "Token ring"
types
  interchangeable node is one of ann, ben, cid
state
  has is a map from node to a boolean
  done is a boolean
initially
  has of every node is false
  done is false
eventually done
action grab(n is a node)
  requires every m in node satisfies not has of m
  has of n becomes true
action pass(from is a node, to is a node)
  requires has of from and not has of to
  has of from becomes false
  has of to becomes true
"#;
    let error = assert_agree(source).expect("a violation");
    assert!(error.message.contains("can run forever"), "{error}");
    let trace = error.trace.expect("a counterexample");
    assert!(trace.loop_from.is_some());
    let symmetric = reduced(source).unwrap_err();
    assert_eq!(symmetric.message, error.message);
}

#[test]
fn liveness_under_fairness_turns_reduction_off() {
    let source = MUTEX.replace("action leave", "fair action leave")
        + "\nwhenever holder is not nobody eventually holder is nobody\n";
    let verification = reduced(&source).unwrap();
    assert!(!verification.reduced);
    assert_eq!(verification.graph.state_count(), 48);
    assert!(
        verification
            .notes
            .iter()
            .any(|note| note.contains("symmetry reduction is off")),
        "{:?}",
        verification.notes
    );
}

#[test]
fn scenarios_may_name_interchangeable_values() {
    let verification = reduced(MUTEX).unwrap();
    assert_eq!(verification.scenarios.len(), 1);
    let broken = MUTEX.replace("reject enter(ben)", "enter(ben)");
    let error = reduced(&broken).unwrap_err();
    assert!(
        error.message.contains("enter(ben) is not allowed"),
        "{error}"
    );
    let trace = error.trace.expect("the scenario so far");
    assert_eq!(trace.steps.len(), 3);
}

#[test]
fn the_specification_cannot_single_out_an_interchangeable_value() {
    for (needle, replacement, line) in [
        ("  holder is nobody\n", "  holder is ann\n", 18),
        (
            "requires at of w is idle",
            "requires at of w is idle and w is not ann",
            29,
        ),
        (
            "always possible holder is nobody",
            "always possible holder is \"ben\"",
            26,
        ),
    ] {
        let error = full(&MUTEX.replacen(needle, replacement, 1)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Spec);
        assert_eq!(error.line, Some(line), "{error}");
        assert!(error.message.contains("cannot single it out"), "{error}");
    }
}

#[test]
fn an_enumeration_holds_all_interchangeable_values_or_none() {
    let error = full(&MUTEX.replace(
        "holder is one of ann, ben, cid, dot, nobody",
        "holder is one of ann, ben, nobody",
    ))
    .unwrap_err();
    assert_eq!(error.line, Some(14));
    assert!(error.message.contains("ann, ben, cid, dot"), "{error}");

    let error = full(&MUTEX.replace(
        "interchangeable worker is one of ann, ben, cid, dot",
        "interchangeable worker is an integer from 1 to 4",
    ))
    .unwrap_err();
    assert!(error.message.contains("only an enumeration"), "{error}");
}

#[test]
fn short_circuiting_quantifiers_with_partial_bodies_use_concrete_evaluation() {
    for workers in ["ann, ben", "ben, ann"] {
        for (expr, kind) in [
            ("1 / (1 - load of w) > 0", "division by zero"),
            ("9223372036854775807 + load of w > 0", "integer overflow"),
            ("valid of (load of w) is true", "is not a key"),
        ] {
            let model = format!(
                "system \"Workers\"\ntypes\n  interchangeable worker is one of {workers}\n  zero is an integer from 0 to 0\nstate\n  load is a map from worker to an integer from 0 to 1\n  valid is a map from zero to a boolean\ninitially\n  load of every worker is 0\n  valid of every zero is true\nalways some w in worker satisfies {expr}\naction start(w is a worker)\n  requires every v in worker satisfies load of v is 0\n  load of w becomes 1\n"
            );
            let error = reduced(&model).unwrap_err();
            assert_eq!(error, full(&model).unwrap_err());
            assert!(error.message.contains(kind), "{error}");
            assert_eq!(error.trace.unwrap().steps.len(), 1);
            let safe = model.replace("load of w becomes 1", "load of w becomes 0");
            let verification = reduced(&safe).unwrap();
            assert!(!verification.reduced);
            assert!(
                verification
                    .notes
                    .iter()
                    .any(|note| note.contains("quantifier"))
            );
        }
    }
}

#[test]
fn model_fault_names_the_same_concrete_worker_as_its_prefix() {
    let model = r#"system "Climb"
types
  interchangeable worker is one of ann, ben
state
  load is a map from worker to an integer from 0 to 1
initially
  load of every worker is 0
action increment(w is a worker)
  load of w becomes load of w + 1
"#;
    let error = reduced(model).unwrap_err();
    assert_eq!(error, full(model).unwrap_err());
    assert!(
        error
            .message
            .starts_with("increment(ann): load of ann would become 2")
    );
    assert_eq!(error.trace.unwrap().steps[0].call, "increment(ann)");
}
