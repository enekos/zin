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

fn model(expected: &State) -> String {
    let (mut types, mut fields, mut initial) = (String::new(), String::new(), String::new());
    for (name, item) in expected {
        let kind = match item {
            Value::Boolean(_) => "a boolean",
            Value::Integer(_) => "an integer from 0 to 1",
            Value::Symbol(_) => "one of open, closed",
            Value::Map(entries) => {
                let keys = entries.keys().cloned().collect::<Vec<_>>().join(", ");
                types = format!("types\n  key is one of {keys}\n");
                for (key, v) in entries {
                    initial.push_str(&format!("  {name} of {key} is {}\n", v.source()));
                }
                "a map from key to a boolean"
            }
        };
        fields.push_str(&format!("  {name} is {kind}\n"));
        if !matches!(item, Value::Map(_)) {
            initial.push_str(&format!("  {name} is {}\n", item.source()));
        }
    }
    format!(
        "system \"Observer\"\n{types}state\n{fields}initially\n{initial}action tick\n  requires true\n"
    )
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
