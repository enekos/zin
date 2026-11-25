use zin::conform::{Adapter, Args, Outcome, check};
use zin::{Json, PlanOptions, State, Value, check_trace, parse, verify};

struct Observer(State);
impl Adapter for Observer {
    fn call(&mut self, _: &str, _: &Args) -> Outcome {
        Outcome::Accepted
    }
    fn observe(&self) -> Option<State> {
        Some(self.0.clone())
    }
}

fn value(json: &Json) -> Value {
    match json {
        Json::Int(n) => Value::Integer(*n),
        Json::Bool(b) => Value::Boolean(*b),
        Json::Str(s) => Value::Symbol(s.clone()),
        Json::Object(_) => Value::Map(state(json)),
        Json::Null => Value::Nothing,
        _ => panic!("unexpected fixture value {json:?}"),
    }
}

fn state(json: &Json) -> State {
    let Json::Object(entries) = json else {
        panic!("expected state object")
    };
    entries
        .iter()
        .map(|(key, item)| (key.clone(), value(item)))
        .collect()
}

/// The declared type of a fixture value. Objects become records, which match
/// observations the way maps do: a subset of the keys, every key known.
fn kind(item: &Value) -> String {
    match item {
        Value::Boolean(_) => "a boolean".into(),
        Value::Integer(_) => "an integer from 0 to 2".into(),
        Value::Nothing => "an integer from 0 to 2 or nothing".into(),
        Value::Symbol(_) => format!("one of {}, other", item.source()),
        Value::Map(entries) => format!(
            "a record with ({})",
            entries
                .iter()
                .map(|(member, value)| format!("{member} is {}", kind(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn model(expected: &State) -> String {
    let (mut fields, mut initial) = (String::new(), String::new());
    for (name, item) in expected {
        fields.push_str(&format!("  {name} is {}\n", kind(item)));
        initial.push_str(&format!("  {name} is {}\n", item.source()));
    }
    format!(
        "system \"Observer\"\nstate\n{fields}initially\n{initial}action tick\n  requires true\n"
    )
}

/// Whether the Rust value model has every kind of value in `json` yet.
fn representable(json: &Json) -> bool {
    match json {
        Json::Object(entries) => entries.iter().all(|(_, item)| representable(item)),
        Json::Int(_) | Json::Bool(_) | Json::Str(_) | Json::Null => true,
        _ => false,
    }
}

#[test]
fn rust_and_trace_replay_share_the_runner_observation_contract() {
    let Json::Array(cases) = Json::parse(include_str!(
        "../bindings/fixtures/contract/observations.json"
    ))
    .unwrap() else {
        panic!("expected fixture array");
    };
    for case in cases {
        if !representable(&case) {
            continue;
        }
        let src = model(&state(case.get("expected").unwrap()));
        let observed = case.get("observed").unwrap();
        let matches = case.get("matches") == Some(&Json::Bool(true));
        let report = check(&src, &PlanOptions::default(), || Observer(state(observed)));
        assert_eq!(report.is_ok(), matches, "{case:?}: {report:?}");
        let checked = verify(&parse(&src).unwrap()).unwrap();
        let trace = format!(
            "{{\"action\":\"tick\",\"state\":{}}}",
            observed.to_compact()
        );
        assert_eq!(
            check_trace(&checked.model, &trace).is_ok(),
            matches,
            "{case:?}"
        );
    }
}
