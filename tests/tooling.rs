use std::collections::BTreeSet;
use zin::conform::{Adapter, Args, Outcome, assert_conforms, check};
use zin::{
    Cover, ErrorKind, Framework, Json, MutantFate, Plan, PlanOptions, State, Value, Verification,
    audit, check_trace, parse, scaffold, verify,
};

const ACCOUNT: &str = include_str!("../examples/account.zin");
const COUNTER: &str = include_str!("../examples/counter.zin");
const DELIVERY: &str = include_str!("../examples/delivery.zin");
const JOB: &str = include_str!("../examples/job.zin");

fn verified(source: &str) -> Verification {
    verify(&parse(source).unwrap()).unwrap()
}

fn plan(source: &str, cover: Cover, max_steps: usize) -> (Verification, Plan) {
    let verification = verified(source);
    let plan = Plan::build(&verification, &PlanOptions { cover, max_steps }).unwrap();
    (verification, plan)
}

#[test]
fn transition_cover_exercises_every_transition_and_every_refusal() {
    for source in [ACCOUNT, COUNTER, DELIVERY, JOB] {
        for max_steps in [1, 7, 200] {
            let (verification, plan) = plan(source, Cover::Transitions, max_steps);
            let graph = &verification.graph;
            let mut accepted = BTreeSet::new();
            let mut refused = BTreeSet::new();
            for test in &plan.tests {
                let mut state = 0;
                for step in &test.steps {
                    // Every step must agree with the model it was derived from.
                    assert_eq!(graph.target(state, step.call), step.to, "{}", test.name);
                    match step.to {
                        Some(to) => {
                            accepted.insert((state, step.call));
                            state = to;
                        }
                        None => {
                            refused.insert((state, step.call));
                        }
                    }
                }
            }
            let calls = u32::try_from(verification.model.call_count()).unwrap();
            for state in 0..u32::try_from(graph.state_count()).unwrap() {
                for call in 0..calls {
                    let covered = match graph.target(state, call) {
                        Some(_) => accepted.contains(&(state, call)),
                        None => refused.contains(&(state, call)),
                    };
                    assert!(
                        covered,
                        "{}: state {state} call {} not covered (max_steps {max_steps})",
                        verification.model.name,
                        verification.call_text(call)
                    );
                }
            }
        }
    }
}

#[test]
fn weaker_covers_are_smaller_and_still_valid() {
    let (_, full) = plan(ACCOUNT, Cover::Transitions, 200);
    for cover in [Cover::States, Cover::Actions, Cover::Scenarios] {
        let (verification, smaller) = plan(ACCOUNT, cover, 200);
        assert!(
            smaller.accepted_steps + smaller.rejected_steps
                < full.accepted_steps + full.rejected_steps,
            "{cover:?}"
        );
        for test in &smaller.tests {
            let mut state = 0;
            for step in &test.steps {
                assert_eq!(verification.graph.target(state, step.call), step.to);
                state = step.to.unwrap_or(state);
            }
        }
    }
    let (verification, states) = plan(ACCOUNT, Cover::States, 200);
    let mut visited: BTreeSet<u32> = states
        .tests
        .iter()
        .flat_map(|test| &test.steps)
        .filter_map(|step| step.to)
        .collect();
    visited.insert(0);
    assert_eq!(visited.len(), verification.graph.state_count());
}

#[test]
fn plan_json_round_trips_and_is_self_contained() {
    let (verification, plan) = plan(COUNTER, Cover::Transitions, 200);
    let text = plan.to_json(&verification).to_compact();
    let json = Json::parse(&text).unwrap();
    assert_eq!(json.get("zin").and_then(Json::as_str), Some("plan/1"));
    assert_eq!(
        json.get("fingerprint").and_then(Json::as_str),
        Some(verification.model.fingerprint.as_str())
    );
    let Some(Json::Array(states)) = json.get("states") else {
        panic!("states");
    };
    let Some(Json::Array(parents)) = json.get("parents") else {
        panic!("parents");
    };
    assert_eq!(states.len(), parents.len());
    assert_eq!(parents.first(), Some(&Json::Null));
    let Some(Json::Array(tests)) = json.get("tests") else {
        panic!("tests");
    };
    assert_eq!(tests.len(), plan.tests.len());
    assert_eq!(json.to_compact(), text);
}

// --- conformance against Rust implementations -------------------------------

#[derive(Default)]
struct Account {
    balance: i64,
    closed: bool,
    /// Which bug, if any, this implementation has.
    bug: Option<&'static str>,
}

impl Account {
    fn deposit(&mut self, amount: i64) -> Result<(), ()> {
        let cap = if self.bug == Some("cap") { 21 } else { 20 };
        if self.closed || self.balance + amount > cap {
            return Err(());
        }
        self.balance += amount;
        Ok(())
    }

    fn withdraw(&mut self, amount: i64) -> Result<(), ()> {
        if self.closed || amount > self.balance {
            return Err(());
        }
        self.balance -= amount;
        Ok(())
    }

    fn close(&mut self) -> Result<(), ()> {
        if self.closed || (self.balance != 0 && self.bug != Some("close")) {
            return Err(());
        }
        self.closed = true;
        Ok(())
    }

    fn reopen(&mut self) -> Result<(), ()> {
        if !self.closed {
            return Err(());
        }
        self.closed = false;
        Ok(())
    }
}

struct Harness {
    account: Account,
    observable: bool,
}

fn amount(args: &Args) -> i64 {
    match args.get("amount") {
        Some(Value::Integer(amount)) => *amount,
        other => panic!("amount should be an integer, got {other:?}"),
    }
}

impl Adapter for Harness {
    fn call(&mut self, action: &str, args: &Args) -> Outcome {
        match action {
            "deposit" => self.account.deposit(amount(args)).into(),
            "withdraw" => self.account.withdraw(amount(args)).into(),
            "close" => self.account.close().into(),
            "reopen" => self.account.reopen().into(),
            other => panic!("unknown action {other}"),
        }
    }

    fn observe(&self) -> Option<State> {
        self.observable.then(|| {
            State::from([
                ("balance".to_string(), Value::Integer(self.account.balance)),
                (
                    "status".to_string(),
                    Value::Symbol(
                        if self.account.closed {
                            "closed"
                        } else {
                            "open"
                        }
                        .to_string(),
                    ),
                ),
            ])
        })
    }
}

fn harness(bug: Option<&'static str>, observable: bool) -> impl FnMut() -> Harness {
    move || Harness {
        account: Account {
            bug,
            ..Account::default()
        },
        observable,
    }
}

#[test]
fn a_correct_implementation_conforms() {
    let conformance = assert_conforms(ACCOUNT, harness(None, true));
    assert!(conformance.accepted_steps >= 182);
    assert!(conformance.rejected_steps > 0);
    assert_conforms(ACCOUNT, harness(None, false));
}

#[test]
fn an_off_by_one_is_caught_even_without_observing_state() {
    for observable in [true, false] {
        let error = check(
            ACCOUNT,
            &PlanOptions::default(),
            harness(Some("cap"), observable),
        )
        .expect_err("the cap bug must be found");
        assert_eq!(error.kind, ErrorKind::Violation);
        assert!(
            error.message.contains("accepted deposit(")
                && error.message.contains("but the specification rejects it"),
            "{error}"
        );
        assert!(error.message.contains("steps taken:"), "{error}");
    }
}

#[test]
fn a_missing_guard_is_caught_with_a_short_reproduction() {
    let error = check(
        ACCOUNT,
        &PlanOptions::default(),
        harness(Some("close"), true),
    )
    .expect_err("closing a funded account must be found");
    assert!(error.message.contains("accepted close"), "{error}");
    if let Some((_, reproduction)) = error.message.split_once("shortest reproduction:\n") {
        assert!(reproduction.lines().count() <= 2, "{error}");
    }
}

#[test]
fn a_wrong_observation_is_caught() {
    struct Liar(Harness);
    impl Adapter for Liar {
        fn call(&mut self, action: &str, args: &Args) -> Outcome {
            self.0.call(action, args)
        }
        fn observe(&self) -> Option<State> {
            let mut state = self.0.observe()?;
            if self.0.account.balance == 7 {
                state.insert("balance".to_string(), Value::Integer(8));
            }
            Some(state)
        }
    }
    let mut make = harness(None, true);
    let error = check(ACCOUNT, &PlanOptions::default(), || Liar(make()))
        .expect_err("the wrong balance must be found");
    assert!(
        error
            .message
            .contains("balance is 8, but the specification says 7"),
        "{error}"
    );
}

// --- traces -----------------------------------------------------------------

#[test]
fn traces_are_checked_event_by_event() {
    let model = verified(ACCOUNT).model;
    let good = r#"{"action":"deposit","args":[5],"outcome":"accepted","state":{"balance":5}}
{"action":"withdraw","args":{"amount":3},"state":{"balance":2,"status":"open"}}

{"action":"close","outcome":"rejected"}
{"reset":true}
{"action":"close"}
{"action":"deposit","args":[1],"outcome":"rejected"}
"#;
    let report = check_trace(&model, good).unwrap();
    assert_eq!((report.events, report.runs), (5, 2));

    for (events, kind, needle) in [
        (
            r#"{"action":"close"}
{"action":"deposit","args":[1]}"#,
            ErrorKind::Violation,
            "trace line 2",
        ),
        (
            r#"{"action":"deposit","args":[5],"state":{"balance":4}}"#,
            ErrorKind::Violation,
            "balance",
        ),
        (
            r#"{"action":"withdraw","args":[1],"outcome":"rejected"}
{"action":"deposit","args":[5],"outcome":"rejected"}"#,
            ErrorKind::Violation,
            "trace line 2",
        ),
        (r#"{"action":"explode"}"#, ErrorKind::Usage, "explode"),
        (
            r#"{"action":"deposit","args":[99]}"#,
            ErrorKind::Usage,
            "trace line 1",
        ),
        ("not json", ErrorKind::Usage, "trace line 1"),
    ] {
        let error = check_trace(&model, events).expect_err(events);
        assert_eq!(error.kind, kind, "{events}: {error}");
        assert!(error.message.contains(needle), "{events}: {error}");
    }
}

// --- audit ------------------------------------------------------------------

#[test]
fn shipped_examples_have_no_surviving_mutants() {
    for source in [ACCOUNT, COUNTER, DELIVERY, JOB] {
        let audit = audit(&parse(source).unwrap()).unwrap();
        assert!(audit.caught() > 5);
        assert!(
            audit.survivors().is_empty(),
            "{:?}",
            audit
                .survivors()
                .iter()
                .map(|mutant| &mutant.description)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn audit_points_at_guards_nothing_checks() {
    let spec = r#"system "Light"
state
  on is a boolean
  uses is an integer from 0 to 3
initially
  on is false
  uses is 0
action switch_on
  requires not on
  requires uses < 3
  on becomes true
  uses becomes uses + 1
action switch_off
  requires on
  on becomes false
"#;
    let audit = audit(&parse(spec).unwrap()).unwrap();
    let survivors = audit.survivors();
    assert!(
        survivors
            .iter()
            .any(|mutant| mutant.line == 9 && mutant.description.contains("drop")),
        "{survivors:?}"
    );
    assert!(
        audit
            .mutants
            .iter()
            .any(|mutant| matches!(mutant.fate, MutantFate::Caught(_)))
    );
}

// --- scaffolds and the command line ----------------------------------------

#[test]
fn scaffolds_name_every_action() {
    let model = verified(ACCOUNT).model;
    for framework in [
        Framework::Pytest,
        Framework::Unittest,
        Framework::Jest,
        Framework::Vitest,
        Framework::Node,
        Framework::Playwright,
        Framework::TestingLibrary,
        Framework::Go,
        Framework::Rust,
    ] {
        let text = scaffold(&model, "account.zin", framework);
        for action in ["deposit", "withdraw", "close", "reopen"] {
            assert!(text.contains(action), "{framework:?}: {text}");
        }
        assert!(text.contains("account.zin"), "{framework:?}");
    }
}

#[test]
fn browser_scaffolds_use_their_own_entry_points() {
    let model = verified(ACCOUNT).model;
    let playwright = scaffold(&model, "account.zin", Framework::Playwright);
    assert!(
        playwright.contains("from \"zin-verify/playwright\""),
        "{playwright}"
    );
    assert!(playwright.contains("async ({ page }) =>"), "{playwright}");
    assert!(
        playwright.contains("async deposit({ amount })"),
        "{playwright}"
    );
    assert!(playwright.ends_with("}, { test });\n"), "{playwright}");

    let component = scaffold(&model, "account.zin", Framework::TestingLibrary);
    assert!(component.contains("@testing-library/react"), "{component}");
    assert!(
        component.contains("join(import.meta.dirname, \"account.zin\")"),
        "{component}"
    );
    assert!(component.contains("      close() {"), "{component}");
    assert_eq!(
        cli(&["init", "testing-library", "examples/account.zin"]).code,
        0
    );
}

fn cli(args: &[&str]) -> zin::cli::Output {
    let args: Vec<String> = args.iter().map(ToString::to_string).collect();
    zin::cli::run(&args)
}

#[test]
fn command_line_exit_codes_separate_wrong_from_unusable() {
    let ok = cli(&["check", "examples/account.zin", "examples/job.zin"]);
    assert_eq!(ok.code, 0, "{}", ok.stderr);
    assert_eq!(ok.stdout.lines().count(), 2);

    let wrong = cli(&["check", "examples/broken/lost-update.zin"]);
    assert_eq!(wrong.code, 1);
    assert!(wrong.stderr.contains("counterexample:"), "{}", wrong.stderr);

    for unusable in [
        cli(&[]),
        cli(&["frobnicate"]),
        cli(&["check"]),
        cli(&["check", "examples/missing.zin"]),
        cli(&["check", "Cargo.toml"]),
        cli(&["plan", "examples/account.zin", "--cover", "everything"]),
        cli(&["plan", "examples/account.zin", "--max-steps", "0"]),
        cli(&["init", "cobol", "examples/account.zin"]),
        cli(&["diagram", "examples/account.zin", "No such scenario"]),
    ] {
        assert_eq!(unusable.code, 2, "{unusable:?}");
        assert!(unusable.stdout.is_empty());
    }
    assert_eq!(cli(&["--version"]).code, 0);
    assert_eq!(cli(&["--help"]).code, 0);
}

#[test]
fn command_line_json_reports_failures_per_file() {
    let output = cli(&[
        "check",
        "examples/account.zin",
        "examples/broken/stuck-job.zin",
        "--format",
        "json",
    ]);
    assert_eq!(output.code, 1);
    let json = Json::parse(&output.stdout).unwrap();
    let Some(Json::Array(results)) = json.get("results") else {
        panic!("{}", output.stdout);
    };
    assert_eq!(
        results.first().and_then(|r| r.get("ok")),
        Some(&Json::Bool(true))
    );
    let failed = results.get(1).unwrap();
    assert_eq!(failed.get("ok"), Some(&Json::Bool(false)));
    let trace = failed.get("error").and_then(|e| e.get("trace")).unwrap();
    assert_eq!(trace.get("loop_from"), Some(&Json::Int(0)));
}

#[test]
fn command_line_plan_report_and_diagram_render() {
    let plan = cli(&["plan", "examples/delivery.zin", "--cover", "states"]);
    assert_eq!(plan.code, 0, "{}", plan.stderr);
    assert_eq!(
        Json::parse(&plan.stdout)
            .unwrap()
            .get("cover")
            .and_then(Json::as_str),
        Some("states")
    );
    let report = cli(&["report", "examples/delivery.zin"]);
    assert!(
        report.stdout.contains("```mermaid\nstateDiagram-v2"),
        "{}",
        report.stdout
    );
    assert!(report.stdout.contains("sequenceDiagram"));
    let diagram = cli(&[
        "diagram",
        "examples/delivery.zin",
        "An unpaid order can be cancelled",
    ]);
    assert!(
        diagram.stdout.starts_with("sequenceDiagram"),
        "{}",
        diagram.stdout
    );
    assert_eq!(cli(&["audit", "examples/job.zin"]).code, 0);
}
