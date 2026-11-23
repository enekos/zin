use zin::{
    Cover, ErrorKind, Json, Plan, PlanOptions, State, TestKind, Verification, adequacy, parse,
    verify,
};

const ACCOUNT: &str = include_str!("../examples/account.zin");
const COUNTER: &str = include_str!("../examples/counter.zin");
const EDITOR: &str = include_str!("../examples/editor.zin");
const MUTEX: &str = include_str!("../examples/mutex.zin");

const UNUSED_FIELD: &str = "\
system \"Door\"

state
  open is a boolean
  visits is an integer from 0 to 2

initially
  open is false
  visits is 0

action push
  requires not open
  open becomes true

action enter
  requires open
  requires visits is less than 2
  open becomes false
  visits becomes visits + 1

action slam
  requires open
  open becomes false
";

fn verified(source: &str) -> Verification {
    verify(&parse(source).unwrap()).unwrap()
}

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_string()).collect()
}

/// What a runner observing `fields` sees in each state.
fn observations(verification: &Verification, fields: &[String]) -> Vec<State> {
    (0..verification.graph.state_count())
        .map(|state| {
            let mut full = verification.state(u32::try_from(state).unwrap());
            full.retain(|name, _| fields.contains(name));
            full
        })
        .collect()
}

/// An implementation that follows the specification except that one
/// transition lands in another state with the same observation.
struct Misrouted {
    from: u32,
    call: u32,
    lands: u32,
}

impl Misrouted {
    /// Whether some test of the plan fails against this implementation, as
    /// seen by a runner that compares outcomes and the observed fields.
    fn caught(&self, verification: &Verification, plan: &Plan, seen: &[State]) -> bool {
        let graph = &verification.graph;
        plan.tests.iter().any(|test| {
            let mut actual = 0;
            test.steps.iter().any(|step| {
                let next = if actual == self.from && step.call == self.call {
                    Some(self.lands)
                } else {
                    graph.target(actual, step.call)
                };
                match (step.to, next) {
                    (None, None) => false,
                    (Some(to), Some(next)) => {
                        actual = next;
                        seen[to as usize] != seen[next as usize]
                    }
                    _ => true,
                }
            })
        })
    }
}

/// Every way a transition the plan's cover takes could land in a look-alike state.
fn misroutes(verification: &Verification, seen: &[State]) -> Vec<Misrouted> {
    let graph = &verification.graph;
    let states = u32::try_from(graph.state_count()).unwrap();
    let mut all = Vec::new();
    for from in 0..states {
        for edge in graph.out(from) {
            for lands in (0..states).filter(|lands| *lands != edge.to) {
                if seen[lands as usize] == seen[edge.to as usize] {
                    all.push(Misrouted {
                        from,
                        call: edge.call,
                        lands,
                    });
                }
            }
        }
    }
    all
}

#[test]
fn an_observing_plan_catches_every_transition_into_a_look_alike_state() {
    for (source, observed) in [
        (EDITOR, fields(&["draft", "status"])),
        (EDITOR, fields(&["status", "saved"])),
        (ACCOUNT, fields(&[])),
        (ACCOUNT, fields(&["status"])),
        (MUTEX, fields(&[])),
        (MUTEX, fields(&["holder"])),
    ] {
        let verification = verified(source);
        assert!(adequacy(&verification, &observed).unwrap().adequate());
        let options = PlanOptions::default();
        let plain = Plan::build(&verification, &options).unwrap();
        let observing = Plan::build_observing(&verification, &options, &observed).unwrap();
        let seen = observations(&verification, &observed);
        let faults = misroutes(&verification, &seen);
        let missed = |plan: &Plan| {
            faults
                .iter()
                .filter(|fault| !fault.caught(&verification, plan, &seen))
                .count()
        };
        let (before, after) = (missed(&plain), missed(&observing));
        if std::env::var_os("ZIN_SHOW_MISROUTES").is_some() {
            let steps = |plan: &Plan| plan.accepted_steps + plan.rejected_steps;
            eprintln!(
                "{} {observed:?}: {} misroutes, missed {before} -> {after}, steps {} -> {}",
                verification.model.name,
                faults.len(),
                steps(&plain),
                steps(&observing)
            );
        }
        assert!(before > 0, "{observed:?}: nothing to identify");
        assert_eq!(after, 0, "{observed:?}: of {}", faults.len());
        let identification = observing.identification.as_ref().unwrap();
        assert!(identification.unconfirmed > 0);
        assert_eq!(identification.blind_states, 0);
    }
}

#[test]
fn identify_tests_follow_the_model_and_leave_the_cover_untouched() {
    let verification = verified(EDITOR);
    let options = PlanOptions::cover(Cover::States);
    let plain = Plan::build(&verification, &options).unwrap();
    let observing = Plan::build_observing(&verification, &options, &fields(&["draft"])).unwrap();
    let added = observing.identification.as_ref().unwrap().tests;
    assert!(added > 0);
    assert_eq!(observing.tests.len(), plain.tests.len() + added);
    for (before, after) in plain.tests.iter().zip(&observing.tests) {
        assert_eq!(before.name, after.name);
        assert_eq!(before.steps, after.steps);
    }
    for test in observing.tests.iter().skip(plain.tests.len()) {
        assert_eq!(test.kind, TestKind::Identify);
        let mut state = 0;
        for step in &test.steps {
            assert_eq!(
                verification.graph.target(state, step.call),
                step.to,
                "{}",
                test.name
            );
            state = step.to.unwrap_or(state);
        }
    }
    let steps: usize = observing.tests.iter().map(|test| test.steps.len()).sum();
    assert_eq!(steps, observing.accepted_steps + observing.rejected_steps);
}

#[test]
fn observing_every_field_needs_no_identification() {
    for source in [ACCOUNT, COUNTER, EDITOR, MUTEX] {
        let verification = verified(source);
        let all: Vec<String> = verification.state(0).into_keys().collect();
        let report = adequacy(&verification, &all).unwrap();
        assert_eq!(report.observations, report.states);
        assert_eq!(report.depth, 0);
        assert!(report.hardest.is_none());
        assert!(report.adequate());
        let options = PlanOptions::default();
        let plain = Plan::build(&verification, &options).unwrap();
        let observing = Plan::build_observing(&verification, &options, &all).unwrap();
        assert_eq!(observing.tests.len(), plain.tests.len());
        assert_eq!(observing.identification.as_ref().unwrap().unconfirmed, 0);
    }
}

#[test]
fn the_hardest_pair_needs_exactly_the_reported_calls() {
    let verification = verified(EDITOR);
    let report = adequacy(&verification, &fields(&["draft", "status"])).unwrap();
    assert_eq!(report.states, 36);
    assert_eq!(report.observations, 9);
    let hardest = report.hardest.unwrap();
    assert_eq!(hardest.calls.len(), report.depth);
    assert_eq!(report.depth, 3);
    assert_eq!(hardest.first["draft"], hardest.second["draft"]);
    assert_eq!(hardest.first["status"], hardest.second["status"]);
    assert_ne!(hardest.first, hardest.second);
}

#[test]
fn a_field_nothing_depends_on_is_reported_blind_until_observed() {
    let verification = verified(UNUSED_FIELD);
    // `visits` gates `enter`, so outcomes alone tell every state apart.
    assert!(adequacy(&verification, &[]).unwrap().adequate());

    let widened = UNUSED_FIELD.replace("  requires visits is less than 2\n", "");
    let widened = widened.replace("visits becomes visits + 1", "visits becomes 1");
    let verification = verified(&widened);
    let report = adequacy(&verification, &fields(&["open"])).unwrap();
    assert!(!report.adequate());
    assert_eq!(report.blind_groups, 2);
    assert_eq!(report.blind_states, 4);
    for group in &report.blind {
        assert_eq!(group.states, 2);
        assert_eq!(group.differ, ["visits"]);
        assert_eq!(group.examples.len(), 2);
    }
    let plan =
        Plan::build_observing(&verification, &PlanOptions::default(), &fields(&["open"])).unwrap();
    let identification = plan.identification.unwrap();
    assert_eq!(identification.blind_states, 4);
    assert_eq!(identification.tests, 0);
    assert!(
        adequacy(&verification, &fields(&["visits"]))
            .unwrap()
            .adequate()
    );
}

#[test]
fn unknown_fields_and_reduced_graphs_are_usage_errors() {
    let verification = verified(EDITOR);
    let error = adequacy(&verification, &fields(&["drafts"])).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Usage);
    assert!(error.message.contains("draft, saved, sent, status"));
    let error =
        Plan::build_observing(&verification, &PlanOptions::default(), &fields(&["x"])).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Usage);

    let reduced = zin::verify_with(&parse(MUTEX).unwrap(), zin::Options::default()).unwrap();
    assert!(reduced.reduced);
    assert_eq!(adequacy(&reduced, &[]).unwrap_err().kind, ErrorKind::Usage);
}

#[test]
fn only_observing_plans_carry_the_observe_key() {
    let verification = verified(EDITOR);
    let options = PlanOptions::default();
    let plain = Plan::build(&verification, &options).unwrap();
    assert!(plain.to_json(&verification).get("observe").is_none());
    assert!(!plain.summary().contains("observing"));

    let observing =
        Plan::build_observing(&verification, &options, &fields(&["status", "draft"])).unwrap();
    let json = observing.to_json(&verification);
    let Some(Json::Array(observed)) = json.get("observe") else {
        panic!("no observe key");
    };
    let names: Vec<&str> = observed.iter().filter_map(Json::as_str).collect();
    assert_eq!(names, ["draft", "status"]);
    assert!(observing.summary().contains("observing draft, status"));

    let nothing = Plan::build_observing(&verification, &options, &[]).unwrap();
    assert_eq!(
        nothing.to_json(&verification).get("observe"),
        Some(&Json::Array(Vec::new()))
    );
}

fn run(args: &[&str]) -> zin::cli::Output {
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    zin::cli::run(&args)
}

#[test]
fn the_observe_command_exits_one_when_states_are_blind() {
    let output = run(&["observe", "examples/account.zin"]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("observing no fields"));
    assert!(output.stdout.contains("every state can be told apart"));
    assert!(output.stdout.contains("zin plan --observe ''"));

    let output = run(&["observe", "examples/counter.zin"]);
    assert_eq!(output.code, 1);
    assert!(output.stdout.contains("BLIND 2 states"));
    assert!(output.stdout.contains("they differ in seen"));
    assert!(output.stdout.contains("2 states in 1 blind group"));

    let output = run(&["observe", "examples/counter.zin", "--fields", "seen"]);
    assert_eq!(output.code, 0, "{}", output.stdout);

    let output = run(&["observe", "examples/counter.zin", "--fields", "nope"]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("'nope' is not a state field"));
}

#[test]
fn the_plan_command_adds_identify_tests_and_warns_about_blind_states() {
    let output = run(&["plan", "examples/editor.zin", "--observe", "draft, status"]);
    assert_eq!(output.code, 0);
    assert!(output.stderr.is_empty());
    let json = Json::parse(&output.stdout).unwrap();
    let Some(Json::Array(tests)) = json.get("tests") else {
        panic!("no tests");
    };
    assert!(
        tests
            .iter()
            .any(|test| test.get("kind").and_then(Json::as_str) == Some("identify"))
    );

    let output = run(&["plan", "examples/counter.zin", "--observe", ""]);
    assert_eq!(output.code, 0);
    assert!(
        output
            .stderr
            .starts_with("warning: 2 states cannot be told")
    );

    let plain = run(&["plan", "examples/editor.zin"]);
    assert!(!plain.stdout.contains("identify"));
    assert!(!plain.stdout.contains("\"observe\""));
}
