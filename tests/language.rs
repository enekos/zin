use zin::{ErrorKind, parse, verify};

fn check(source: &str) -> zin::Result<zin::Verification> {
    verify(&parse(source)?)
}

fn failure(source: &str) -> zin::Error {
    check(source).expect_err("the specification should be rejected")
}

const LIGHT: &str = r#"system "Light"
state
  on is a boolean
initially
  on is false
action toggle
  on becomes not on
"#;

#[test]
fn shipped_examples_verify() {
    for (name, source) in [
        ("account", include_str!("../examples/account.zin")),
        ("counter", include_str!("../examples/counter.zin")),
        ("delivery", include_str!("../examples/delivery.zin")),
        ("job", include_str!("../examples/job.zin")),
        ("mutex", include_str!("../examples/mutex.zin")),
        (
            "request-reply",
            include_str!("../examples/request-reply.zin"),
        ),
    ] {
        check(source).unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn explores_every_reachable_state() {
    let verification = check(LIGHT).unwrap();
    assert_eq!(verification.graph.state_count(), 2);
    assert_eq!(verification.graph.transition_count(), 2);
}

#[test]
fn safety_violation_reports_the_shortest_counterexample() {
    let error = failure(include_str!("../examples/broken/lost-update.zin"));
    assert_eq!(error.kind, ErrorKind::Violation);
    let trace = error.trace.expect("a counterexample");
    let calls: Vec<&str> = trace.steps.iter().map(|step| step.call.as_str()).collect();
    assert_eq!(calls.len(), 4);
    assert!(
        calls.starts_with(&["read(alice)", "read(bob)"]),
        "{calls:?}"
    );
    assert_eq!(trace.loop_from, None);
}

#[test]
fn liveness_violation_reports_a_lasso() {
    let error = failure(include_str!("../examples/broken/stuck-job.zin"));
    assert_eq!(error.kind, ErrorKind::Violation);
    assert!(error.message.contains("can run forever"), "{error}");
    let trace = error.trace.as_ref().expect("a counterexample");
    assert_eq!(trace.loop_from, Some(0));
    assert!(error.to_string().contains("then forever:"), "{error}");
}

#[test]
fn fairness_rules_out_starving_a_continuously_enabled_action() {
    let spec = |fair: &str| {
        format!(
            r#"system "Spinner"
state
  done is a boolean
  ticks is an integer from 0 to 1
initially
  done is false
  ticks is 0
eventually
  done
terminal when done
action tick
  requires not done
  ticks becomes 1 - ticks
{fair}action finish
  requires not done
  done becomes true
"#
        )
    };
    let error = failure(&spec(""));
    assert!(error.message.contains("can run forever"), "{error}");
    check(&spec("fair ")).unwrap();
}

#[test]
fn strong_fairness_rules_out_starving_an_action_enabled_on_and_off() {
    let source = include_str!("../examples/broken/stuck-job.zin");
    let error = failure(source);
    assert!(error.message.contains("can run forever"), "{error}");
    let strong = source.replace("fair action succeed", "strongly fair action succeed");
    check(&strong).unwrap();
}

#[test]
fn strong_fairness_still_allows_a_cycle_that_never_enables_the_action() {
    let error = failure(
        r#"system "Detour"
state
  at is one of home, park, shop, done
initially
  at is home
eventually
  at is done
action to_park
  requires at is home
  at becomes park
action to_shop
  requires at is home
  at becomes shop
action back
  requires at is park or at is shop
  at becomes home
strongly fair action finish
  requires at is shop
  at becomes done
"#,
    );
    assert!(error.message.contains("can run forever"), "{error}");
    let trace = error.trace.expect("a counterexample");
    let looped: Vec<&str> = trace
        .steps
        .iter()
        .skip(trace.loop_from.expect("a loop"))
        .map(|step| step.call.as_str())
        .collect();
    assert_eq!(looped, ["to_park", "back"], "{trace}");
}

#[test]
fn strong_fairness_on_a_call_enabled_in_part_of_a_loop_forces_it() {
    let source = |fairness: &str| {
        format!(
            r#"system "Doorbell"
state
  light is a boolean
  answered is a boolean
initially
  light is false
  answered is false
eventually
  answered
terminal when answered
fair action flicker
  requires not answered
  light becomes not light
{fairness}action answer
  requires light and not answered
  answered becomes true
"#
        )
    };
    assert!(failure(&source("")).message.contains("can run forever"));
    assert!(
        failure(&source("fair "))
            .message
            .contains("can run forever")
    );
    check(&source("strongly fair ")).unwrap();
}

#[test]
fn strongly_needs_fair_action() {
    let error = failure(
        r#"system "Typo"
state
  on is a boolean
initially
  on is false
strongly action toggle
  on becomes not on
"#,
    );
    assert_eq!(error.line, Some(6));
    assert!(
        error
            .message
            .contains("expected 'strongly fair action <name>'"),
        "{error}"
    );
}

#[test]
fn stopping_before_the_goal_violates_eventually() {
    let error = failure(
        r#"system "Stops short"
state
  step is an integer from 0 to 2
initially
  step is 0
eventually
  step is 2
action advance
  requires step is 0
  step becomes 1
"#,
    );
    assert!(error.message.contains("stop"), "{error}");
    assert_eq!(error.trace.expect("a counterexample").steps.len(), 1);
}

#[test]
fn leads_to_holds_only_when_every_premise_state_recovers() {
    let spec = |recover: &str| {
        format!(
            r#"system "Alarm"
state
  alarm is one of quiet, ringing
initially
  alarm is quiet
whenever alarm is ringing eventually alarm is quiet
fair action trip
  requires alarm is quiet
  alarm becomes ringing
{recover}action silence
  requires alarm is ringing
  alarm becomes quiet
"#
        )
    };
    check(&spec("fair ")).unwrap();
    // Without `silence` the alarm rings with nothing left to do.
    let stuck = spec("fair ").replace("  alarm becomes quiet", "  alarm becomes ringing");
    assert_eq!(failure(&stuck).kind, ErrorKind::Violation);
}

#[test]
fn always_possible_finds_a_state_with_no_way_back() {
    let error = failure(
        r#"system "Trapdoor"
state
  room is one of hall, cellar
initially
  room is hall
always possible room is hall
action fall
  requires room is hall
  room becomes cellar
action pace
  requires room is cellar
  room becomes cellar
"#,
    );
    assert_eq!(error.kind, ErrorKind::Violation);
    assert!(error.message.contains("always possible"), "{error}");
}

#[test]
fn sometimes_needs_a_witness() {
    let error = failure(&format!("{LIGHT}sometimes\n  on and not on\n"));
    assert_eq!(error.kind, ErrorKind::Violation);
    assert!(error.message.contains("no reachable state"), "{error}");
}

#[test]
fn deadlock_is_reported_when_terminal_states_are_declared() {
    let error = failure(
        r#"system "Door"
state
  door is one of open, closed, jammed
initially
  door is closed
terminal when door is open
action open
  requires door is closed
  door becomes open
action jam
  requires door is closed
  door becomes jammed
"#,
    );
    assert_eq!(error.kind, ErrorKind::Violation);
    assert!(error.message.contains("deadlock"), "{error}");
}

#[test]
fn out_of_range_assignment_is_a_model_fault_with_a_trace() {
    let error = failure(
        r#"system "Overflow"
state
  n is an integer from 0 to 2
initially
  n is 0
action bump
  n becomes n + 1
"#,
    );
    assert_eq!(error.kind, ErrorKind::Model);
    assert!(error.message.contains("add a 'requires'"), "{error}");
    assert_eq!(error.trace.expect("a trace").steps.len(), 2);
}

#[test]
fn state_limit_is_enforced() {
    let error = failure(
        r#"system "Big"
explore at most 10 states
state
  n is an integer from 0 to 100
initially
  n is 0
action bump
  requires n < 100
  n becomes n + 1
"#,
    );
    assert_eq!(error.kind, ErrorKind::Limit);
}

#[test]
fn maps_quantifiers_and_conditionals_evaluate() {
    let verification = check(
        r#"system "Votes"
types
  voter is one of ann, ben, cat
state
  vote is a map from voter to a boolean
  passed is a boolean
initially
  vote of every voter is false
  passed is false
always
  passed is ((number of v in voter where vote of v) >= 2)
sometimes
  every v in voter satisfies vote[v]
  passed and some v in voter satisfies not vote of v
action approve(v is a voter)
  requires not vote of v
  vote of v becomes true
  passed becomes if (number of w in voter where vote of w) >= 1 then true else false
"#,
    )
    .unwrap();
    assert_eq!(verification.graph.state_count(), 8);
}

#[test]
fn assignments_in_one_action_read_the_state_before_the_action() {
    check(
        r#"system "Swap"
state
  a is an integer from 0 to 1
  b is an integer from 0 to 1
initially
  a is 0
  b is 1
always
  a + b is 1
action swap
  a becomes b
  b becomes a
scenario "Swap twice"
  swap
  expect a is 1 and b is 0
  swap
  expect a is 0 and b is 1
"#,
    )
    .unwrap();
}

#[test]
fn scenario_failures_name_the_scenario() {
    let error = failure(&format!(
        "{LIGHT}scenario \"Wrong\"\n  toggle\n  expect not on\n"
    ));
    assert_eq!(error.kind, ErrorKind::Violation);
    assert!(error.message.contains("Wrong"), "{error}");
    let error = failure(&format!("{LIGHT}scenario \"Refused\"\n  reject toggle\n"));
    assert!(error.message.contains("Refused"), "{error}");
}

#[test]
fn parse_errors_carry_line_numbers() {
    for (source, line, needle) in [
        ("state\n  x is a boolean\n", 1, "system"),
        (
            "system \"S\"\nstate\n  x is a frog\ninitially\n  x is 1\n",
            3,
            "frog",
        ),
        (
            "system \"S\"\nstate\n  x is a boolean\ninitially\n  x is false\naction a\n  y becomes true\n",
            7,
            "y",
        ),
        (
            "system \"S\"\nstate\n  x is a boolean\ninitially\n  x is 3\n",
            5,
            "",
        ),
        (
            "system \"S\"\nstate\n  x is a boolean\n  x is a boolean\ninitially\n  x is false\n",
            4,
            "x",
        ),
    ] {
        let error = failure(source);
        assert_eq!(error.kind, ErrorKind::Spec, "{source}: {error}");
        assert_eq!(error.line, Some(line), "{source}: {error}");
        assert!(error.message.contains(needle), "{source}: {error}");
    }
}

#[test]
fn hostile_input_is_rejected_without_panicking() {
    let deep = format!(
        "system \"S\"\nstate\n  x is a boolean\ninitially\n  x is false\nalways\n  {}x{}\n",
        "(".repeat(10_000),
        ")".repeat(10_000)
    );
    assert_eq!(failure(&deep).kind, ErrorKind::Spec);
    for source in [
        "",
        "\u{0}",
        "system",
        "system \"",
        "action",
        "💥",
        "state\n\tx",
    ] {
        let _ = check(source);
    }
}
