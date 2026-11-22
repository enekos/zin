use zin::{Cover, ErrorKind, Plan, PlanOptions, TestKind, Verification, parse, verify};

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

const CHAIN: &str = "system \"Counter\"\nstate\n  n is an integer from 0 to 400\ninitially\n  n is 0\naction advance\n  requires n < 400\n  n becomes n + 1\n";

fn verified(source: &str) -> Verification {
    verify(&parse(source).unwrap()).unwrap()
}

fn options(cover: Cover, step_budget: usize) -> PlanOptions {
    let mut options = PlanOptions::cover(cover);
    options.step_budget = step_budget;
    options
}

#[test]
fn every_step_counts_toward_the_budget_even_in_authored_scenarios() {
    let source = format!("{SWITCH}\nscenario \"cycle\"\n  start\n  reject start\n  stop\n");
    let v = verified(&source);
    for cover in [
        Cover::States,
        Cover::Scenarios,
        Cover::Actions,
        Cover::Transitions,
    ] {
        assert_eq!(
            Plan::build(&v, &options(cover, 2)).unwrap_err().kind,
            ErrorKind::Limit,
            "{cover:?}"
        );
    }
}

#[test]
fn a_deep_chain_is_one_tour_at_the_default_target() {
    let v = verified(CHAIN);
    for cover in [Cover::States, Cover::Transitions] {
        let plan = Plan::build(&v, &PlanOptions::cover(cover)).unwrap();
        assert_eq!(plan.tests.len(), 1, "{cover:?}");
        assert_eq!(plan.accepted_steps, 400, "{cover:?}");
        assert!(plan.accepted_steps + plan.rejected_steps <= 800);
    }
    let exact = Plan::build(&v, &options(Cover::States, 400)).unwrap();
    assert_eq!((exact.tests.len(), exact.accepted_steps), (1, 400));
    assert_eq!(
        Plan::build(&v, &options(Cover::States, 399))
            .unwrap_err()
            .kind,
        ErrorKind::Limit
    );
}

#[test]
fn a_short_target_still_covers_everything_without_restarting_for_each_state() {
    let v = verified(CHAIN);
    let mut short = PlanOptions::cover(Cover::Transitions);
    short.max_steps = 10;
    let plan = Plan::build(&v, &short).unwrap();
    assert_eq!(plan.accepted_steps, 400);
    assert_eq!(plan.tests.len(), 1);
}

#[test]
fn scenario_cover_has_no_generated_witnesses_and_empty_plans_check_the_initial_state() {
    let source = format!("{SWITCH}\nsometimes active\n");
    let v = verified(&source);
    let scenarios = Plan::build(&v, &PlanOptions::cover(Cover::Scenarios)).unwrap();
    assert!(scenarios.tests.is_empty());
    let still = verified(
        "system \"Still\"\nstate\n  x is a boolean\ninitially\n  x is false\naction noop\n  requires x\n",
    );
    let mut actions = PlanOptions::cover(Cover::States);
    actions.max_steps = 1;
    let plan = Plan::build(&still, &actions).unwrap();
    assert_eq!(plan.tests.len(), 1);
    assert_eq!(plan.tests[0].kind, TestKind::Tour);
    assert!(plan.tests[0].steps.is_empty());
}

#[test]
fn plans_record_their_generation_options() {
    let v = verified(SWITCH);
    let mut options = options(Cover::States, 50);
    options.max_steps = 7;
    let json = Plan::build(&v, &options).unwrap().to_json(&v).to_compact();
    assert!(json.contains("\"max_steps\":7"), "{json}");
    assert!(json.contains("\"step_budget\":50"), "{json}");
    assert!(json.contains("\"refusals\":2"), "{json}");
}

#[test]
#[cfg(target_pointer_width = "64")]
fn options_must_be_positive_and_portable() {
    let v = verified(SWITCH);
    let mut huge = PlanOptions::default();
    huge.max_steps = usize::MAX;
    let mut zero = PlanOptions::default();
    zero.step_budget = 0;
    let mut over = PlanOptions::default();
    over.step_budget = usize::MAX;
    for options in [huge, zero, over] {
        assert_eq!(
            Plan::build(&v, &options).unwrap_err().kind,
            ErrorKind::Usage
        );
    }
}
