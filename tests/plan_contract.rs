use std::collections::BTreeSet;
use zin::{
    Cover, ErrorKind, Options, Plan, PlanOptions, audit, check_trace_reader, parse, verify,
    verify_with,
};

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
fn pair_plans_cover_real_adjacent_calls_including_refusals_and_repetitions() {
    let v = verify(&parse(SWITCH).unwrap()).unwrap();
    let options = PlanOptions {
        cover: Cover::Pairs,
        ..Default::default()
    };
    let plan = Plan::build(&v, &options).unwrap();
    let mut pairs = BTreeSet::new();
    for test in &plan.tests {
        let mut state = 0;
        let mut previous = None;
        for step in &test.steps {
            assert_eq!(step.to, v.graph.target(state, step.call));
            if let Some((from, first)) = previous {
                pairs.insert((from, first, step.call));
            }
            previous = Some((state, step.call));
            state = step.to.unwrap_or(state);
        }
    }
    assert_eq!(pairs.len(), 8);
    assert_eq!(plan.covered_pairs, pairs.len());
    assert_eq!(plan.eligible_pairs, 8);
    let sampled = PlanOptions {
        pair_limit: Some(1),
        step_budget: 2,
        ..options
    };
    let a = Plan::build(&v, &sampled).unwrap();
    let b = Plan::build(&v, &sampled).unwrap();
    assert_eq!(a.to_json(&v), b.to_json(&v));
    assert_eq!((a.covered_pairs, a.eligible_pairs), (1, 8));
    assert_eq!(
        Plan::build(
            &v,
            &PlanOptions {
                step_budget: 1,
                ..sampled
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::Limit
    );
}

#[test]
fn every_step_counts_toward_the_budget_even_in_authored_scenarios() {
    let source = format!("{SWITCH}\nscenario \"cycle\"\n  start\n  reject start\n  stop\n");
    let v = verify(&parse(&source).unwrap()).unwrap();
    for cover in [
        Cover::States,
        Cover::Scenarios,
        Cover::Actions,
        Cover::Pairs,
        Cover::Transitions,
    ] {
        let options = PlanOptions {
            cover,
            step_budget: 2,
            ..Default::default()
        };
        assert_eq!(
            Plan::build(&v, &options).unwrap_err().kind,
            ErrorKind::Limit
        );
    }
}

#[test]
fn restarting_a_deep_counter_costs_more_than_finishing_its_tour() {
    let source = "system \"Counter\"\nstate\n  n is an integer from 0 to 400\ninitially\n  n is 0\naction advance\n  requires n < 400\n  n becomes n + 1\n";
    let v = verify(&parse(source).unwrap()).unwrap();
    let options = PlanOptions {
        cover: Cover::States,
        step_budget: 400,
        ..Default::default()
    };
    let p = Plan::build(&v, &options).unwrap();
    assert_eq!((p.tests.len(), p.accepted_steps), (1, 400));
    assert_eq!(
        Plan::build(
            &v,
            &PlanOptions {
                step_budget: 399,
                ..options
            }
        )
        .unwrap_err()
        .kind,
        ErrorKind::Limit
    );
}

#[test]
fn representatives_cannot_become_concrete_plans() {
    let ast = parse(include_str!("../examples/mutex.zin")).unwrap();
    let reduced = verify_with(&ast, Options::default()).unwrap();
    assert!(reduced.reduced);
    assert_eq!(
        Plan::build(&reduced, &PlanOptions::default())
            .unwrap_err()
            .kind,
        ErrorKind::Usage
    );
    assert!(Plan::build(&verify(&ast).unwrap(), &PlanOptions::default()).is_ok());
}

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
    assert!(err.message.contains("line 2"));
}

#[test]
fn audit_gives_a_shortest_distinguishing_example_for_every_survivor() {
    let a = audit(&parse(SWITCH).unwrap()).unwrap();
    let survivors = a.survivors();
    assert!(!survivors.is_empty());
    for m in survivors {
        let w = m.witness.as_ref().unwrap();
        assert_ne!(w.original, w.mutated);
        assert!(w.prefix.len() <= 1);
    }
    let dropped = a
        .mutants
        .iter()
        .find(|m| m.description == "action start: drop 'requires not active'")
        .unwrap();
    let w = dropped.witness.as_ref().unwrap();
    assert_eq!(w.prefix, ["start"]);
    assert_eq!(w.call, "start");
    assert!(w.original.is_none() && w.mutated.is_some());
}

#[test]
#[cfg(target_pointer_width = "64")]
fn cache_options_round_trip_as_portable_json_integers() {
    let v = verify(&parse(SWITCH).unwrap()).unwrap();
    for options in [
        PlanOptions {
            max_steps: usize::MAX,
            ..Default::default()
        },
        PlanOptions {
            cover: Cover::Pairs,
            pair_limit: Some(usize::MAX),
            ..Default::default()
        },
    ] {
        assert_eq!(
            Plan::build(&v, &options).unwrap_err().kind,
            ErrorKind::Usage
        );
    }
}
