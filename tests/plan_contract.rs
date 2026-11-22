use std::collections::BTreeSet;
use zin::conform::{Adapter, Args, Outcome, check};
use zin::{Cover, Coverage, ErrorKind, Plan, PlanOptions, TestKind, Verification, parse, verify};

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

fn adjacent_pairs(v: &Verification, plan: &Plan) -> BTreeSet<(u32, u32, u32)> {
    let mut pairs = BTreeSet::new();
    for test in &plan.tests {
        let mut state = 0;
        let mut previous = None;
        for step in &test.steps {
            assert_eq!(step.to, v.graph.target(state, step.call), "{}", test.name);
            if let Some((from, first)) = previous {
                pairs.insert((from, first, step.call));
            }
            previous = Some((state, step.call));
            state = step.to.unwrap_or(state);
        }
    }
    pairs
}

fn pairs(limit: Option<usize>) -> PlanOptions {
    PlanOptions::cover(Cover::Pairs { limit })
}

#[test]
fn pair_plans_cover_real_adjacent_calls_including_refusals_and_repetitions() {
    let v = verified(SWITCH);
    let plan = Plan::build(&v, &pairs(None)).unwrap();
    let exercised = adjacent_pairs(&v, &plan);
    assert_eq!(exercised.len(), 8);
    assert_eq!(
        plan.coverage,
        Coverage::Pairs {
            eligible: 8,
            covered: 8
        }
    );
    let mut sampled = pairs(Some(1));
    sampled.step_budget = 2;
    let a = Plan::build(&v, &sampled).unwrap();
    let b = Plan::build(&v, &sampled).unwrap();
    assert_eq!(a.to_json(&v).to_compact(), b.to_json(&v).to_compact());
    assert_eq!(a.tests[0].steps.len(), 2);
    assert!(matches!(
        a.coverage,
        Coverage::Pairs {
            eligible: 8,
            covered: 1
        }
    ));
    sampled.step_budget = 1;
    assert_eq!(
        Plan::build(&v, &sampled).unwrap_err().kind,
        ErrorKind::Limit
    );
}

#[test]
fn pair_tours_chain_obligations_instead_of_replaying_a_setup_for_each() {
    let source = CHAIN.replace(
        "  n becomes n + 1\n",
        "  n becomes n + 1\naction reset\n  requires n is 400\n  n becomes 0\n",
    );
    let v = verified(&source);
    let transitions = Plan::build(&v, &PlanOptions::default()).unwrap();
    let plan = Plan::build(&v, &pairs(None)).unwrap();
    let exercised = adjacent_pairs(&v, &plan);
    assert_eq!(exercised.len(), 401 * 4);
    let steps = plan.accepted_steps + plan.rejected_steps;
    let baseline = transitions.accepted_steps + transitions.rejected_steps;
    assert!(steps <= 2 * exercised.len(), "{steps} steps");
    assert!(steps <= 3 * baseline, "{steps} vs {baseline}");
    let mut tight = pairs(None);
    tight.step_budget = 1_000;
    assert_eq!(Plan::build(&v, &tight).unwrap_err().kind, ErrorKind::Limit);
}

#[test]
fn every_test_still_validates_against_the_model_for_each_example() {
    for source in [
        include_str!("../examples/account.zin"),
        include_str!("../examples/delivery.zin"),
        include_str!("../examples/job.zin"),
        include_str!("../examples/request-reply.zin"),
    ] {
        let v = verified(source);
        let plan = Plan::build(&v, &pairs(None)).unwrap();
        let Coverage::Pairs { eligible, covered } = plan.coverage else {
            panic!("pair coverage expected");
        };
        assert_eq!(adjacent_pairs(&v, &plan).len(), covered);
        assert_eq!(covered as u64, eligible, "{}", v.model.name);
    }
}

#[test]
fn a_pair_limit_needs_a_positive_portable_value() {
    let v = verified(SWITCH);
    for limit in [0, usize::MAX] {
        assert_eq!(
            Plan::build(&v, &pairs(Some(limit))).unwrap_err().kind,
            ErrorKind::Usage
        );
    }
}

const ACK: &str = r#"system "Acknowledgement"
state
  open is a boolean
initially
  open is true
action ack
  requires open
  open becomes true
"#;

struct Ack {
    acknowledged: bool,
    forgets: bool,
}

impl Adapter for Ack {
    fn call(&mut self, action: &str, _: &Args) -> Outcome {
        assert_eq!(action, "ack");
        if self.acknowledged && !self.forgets {
            return Outcome::Rejected;
        }
        self.acknowledged = true;
        Outcome::Accepted
    }
}

#[test]
fn pairs_catch_a_history_dependent_refusal_that_transitions_miss() {
    let faulty = || Ack {
        acknowledged: false,
        forgets: false,
    };
    let fixed = || Ack {
        acknowledged: false,
        forgets: true,
    };
    assert!(check(ACK, &PlanOptions::default(), faulty).is_ok());
    let error = check(ACK, &pairs(None), faulty).unwrap_err();
    assert!(error.message.contains("rejected ack"), "{error}");
    assert!(check(ACK, &pairs(None), fixed).is_ok());
}
