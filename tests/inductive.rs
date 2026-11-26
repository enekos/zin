use std::collections::BTreeMap;
use zin::{Broken, ErrorKind, Induction, Json, State, Value, check_induction, parse, verify};

fn induction(source: &str) -> zin::Result<Induction> {
    check_induction(&verify(&parse(source)?)?)
}

fn state(entries: &[(&str, i64)]) -> State {
    entries
        .iter()
        .map(|(name, value)| ((*name).to_string(), Value::Integer(*value)))
        .collect::<BTreeMap<_, _>>()
}

fn cli(args: &[&str]) -> zin::cli::Output {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    zin::cli::run(&args)
}

const LOCKSTEP: &str = r#"system "Lockstep"
state
  a is an integer from 0 to 3
  b is an integer from 0 to 3
initially
  a is 0
  b is 0
always
  b is at most 2
action step
  requires a < 2
  a becomes a + 1
  b becomes b + 1
"#;

#[test]
fn inductive_properties_hold_from_every_satisfying_state() {
    let induction = induction(include_str!("../examples/account.zin")).unwrap();
    assert!(induction.holds());
    assert_eq!(induction.properties, 3);
    assert_eq!(induction.well_typed_states, 42);
    assert_eq!(induction.satisfying_states, 22);
}

#[test]
fn a_true_property_can_fail_to_be_inductive() {
    verify(&parse(LOCKSTEP).unwrap()).expect("b stays at most 2 in every reachable state");
    let induction = induction(LOCKSTEP).unwrap();
    assert!(!induction.holds());
    assert_eq!(induction.well_typed_states, 16);
    assert_eq!(induction.satisfying_states, 12);
    let found = induction.counterexample.unwrap();
    assert_eq!(found.state, state(&[("a", 0), ("b", 2)]));
    assert_eq!(found.call, "step");
    assert_eq!(found.next, Some(state(&[("a", 1), ("b", 3)])));
    assert_eq!(
        found.broken,
        Broken::Property {
            text: "always b is at most 2".to_string(),
            line: 9
        }
    );

    let strengthened = LOCKSTEP.replace("  b is at most 2\n", "  b is at most 2\n  a is b\n");
    let induction = self::induction(&strengthened).unwrap();
    assert!(induction.holds(), "{:?}", induction.counterexample);
    assert_eq!(induction.properties, 2);
    assert_eq!(induction.satisfying_states, 3);
}

#[test]
fn a_fault_from_a_satisfying_state_breaks_induction() {
    let source = LOCKSTEP.replace("b is an integer from 0 to 3", "b is an integer from 0 to 2");
    let found = induction(&source).unwrap().counterexample.unwrap();
    assert_eq!(found.state, state(&[("a", 0), ("b", 2)]));
    assert_eq!(found.next, None);
    let Broken::Fault { message, line } = found.broken else {
        panic!("expected a fault, got {:?}", found.broken);
    };
    assert_eq!(line, 13);
    assert!(message.starts_with("b would become 3"), "{message}");
}

#[test]
fn compound_fields_enumerate_each_value_once() {
    let source = r#"system "Compound"
types
  color is one of red, blue
state
  lease is an integer from 1 to 2 or nothing
  queue is a list of at most 2 color
  job is one of queued, running with (attempt is an integer from 1 to 3), failed
  seen is a set of color
  order is a record with (paid is a boolean, items is an integer from 0 to 2)
  marks is a map from color to an integer from 0 to 1
initially
  lease is nothing
  queue is []
  job is queued
  seen is []
  order is {paid: false, items: 0}
  marks of every color is 0
always
  size of seen <= 2
action clear
  queue becomes []
"#;
    let induction = induction(source).unwrap();
    let optional = 2 + 1;
    let list = 1 + 2 + 2 * 2;
    let variant = 1 + 3 + 1;
    let set = 2 * 2;
    let record = 2 * 3;
    let map = 2 * 2;
    let product = optional * list * variant * set * record * map;
    assert_eq!(induction.well_typed_states, product);
    assert_eq!(induction.satisfying_states, product);
    assert!(induction.holds(), "{:?}", induction.counterexample);
}

#[test]
fn a_domain_past_the_state_limit_is_an_error() {
    let source = r#"system "Wide"
explore at most 100 states
state
  a is an integer from 0 to 20
  b is an integer from 0 to 20
initially
  a is 0
  b is 0
always
  a is b
action step
  requires a < 2
  a becomes a + 1
  b becomes b + 1
"#;
    let error = induction(source).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Limit);
    assert!(error.message.contains("allow 441"), "{}", error.message);
}

#[test]
fn without_safety_properties_there_is_nothing_to_check() {
    let source = r#"system "Light"
state
  on is a boolean
initially
  on is false
sometimes
  on
action toggle
  on becomes not on
"#;
    let induction = induction(source).unwrap();
    assert_eq!(induction.properties, 0);
    assert!(induction.holds());
}

#[test]
fn check_reports_induction_on_request() {
    let plain = cli(&["check", "examples/job.zin"]);
    assert!(!plain.stdout.contains("inductive"), "{}", plain.stdout);

    let text = cli(&[
        "check",
        "--inductive",
        "examples/account.zin",
        "examples/job.zin",
    ]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout
            .contains("inductive: the 3 always/never properties still hold"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("not inductive: start faults (examples/job.zin:31)"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("    state: attempts=3, status=queued\n    start  ->  fault\n"),
        "{}",
        text.stdout
    );

    let json = cli(&[
        "check",
        "--inductive",
        "--format",
        "json",
        "examples/job.zin",
    ]);
    assert_eq!(json.code, 0, "{}", json.stderr);
    let parsed = Json::parse(json.stdout.trim()).unwrap();
    let Some(Json::Array(results)) = parsed.get("results") else {
        panic!("{}", json.stdout);
    };
    let inductive = results[0].get("inductive").unwrap();
    assert_eq!(inductive.get("holds"), Some(&Json::Bool(false)));
    assert_eq!(inductive.get("well_typed_states"), Some(&Json::Int(16)));
    let found = inductive.get("counterexample").unwrap();
    assert_eq!(found.get("call").and_then(Json::as_str), Some("start"));
    assert_eq!(found.get("next"), Some(&Json::Null));
    assert_eq!(found.get("property"), Some(&Json::Null));
    assert_eq!(found.get("line"), Some(&Json::Int(31)));
    assert_eq!(
        found.get("file").and_then(Json::as_str),
        Some("examples/job.zin")
    );
}
