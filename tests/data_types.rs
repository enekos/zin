//! Records, nested maps and the other compound types: checking, plans and
//! conformance against implementations that keep the same shape.

use std::collections::BTreeMap;
use zin::conform::{Adapter, Args, Outcome, check};
use zin::{ErrorKind, Json, Options, Plan, PlanOptions, State, Value, parse, verify, verify_with};

const ORDERS: &str = include_str!("../examples/orders.zin");

fn error(source: &str) -> zin::Error {
    verify(&parse(source).unwrap()).unwrap_err()
}

#[test]
fn records_and_nested_maps_check_with_and_without_symmetry() {
    let full = verify(&parse(ORDERS).unwrap()).unwrap();
    assert_eq!(full.graph.state_count(), 185);
    let reduced = verify_with(&parse(ORDERS).unwrap(), Options::default()).unwrap();
    assert!(reduced.reduced);
    assert_eq!(reduced.graph.state_count(), 94);
    let initial = full.state(0);
    assert_eq!(
        initial.get("last").map(Value::source).as_deref(),
        Some("{what: {items: 0, status: draft}, who: nobody}")
    );
}

#[test]
fn plans_with_records_are_plan_2_and_nest_values() {
    let verification = verify(&parse(ORDERS).unwrap()).unwrap();
    let plan = Plan::build(&verification, &PlanOptions::default()).unwrap();
    let json = plan.to_json(&verification);
    assert_eq!(json.get("zin"), Some(&Json::str("plan/2")));
    let fields = json.get("fields").unwrap().to_compact();
    assert!(
        fields.contains(r#"{"kind":"record","members":[{"name":"status","type":{"kind":"enum","values":["draft","paid","shipped"]}},{"name":"items","type":{"kind":"integer","min":0,"max":2}}]}"#),
        "{fields}"
    );
    assert!(
        fields
            .contains(r#""value":{"kind":"map","keys":["mon","tue"],"value":{"kind":"boolean"}}"#),
        "{fields}"
    );
    let Some(Json::Array(states)) = json.get("states") else {
        panic!("no states")
    };
    assert_eq!(
        states[0].get("orders").unwrap().to_compact(),
        r#"{"ann":{"items":0,"status":"draft"},"ben":{"items":0,"status":"draft"}}"#
    );
}

#[test]
fn plans_without_compound_types_stay_plan_1() {
    let source = include_str!("../examples/mutex.zin");
    let verification = verify(&parse(source).unwrap()).unwrap();
    let plan = Plan::build(&verification, &PlanOptions::default()).unwrap();
    assert_eq!(
        plan.to_json(&verification).get("zin"),
        Some(&Json::str("plan/1"))
    );
}

#[test]
fn every_key_assignments_reach_members() {
    let source = ORDERS.replace(
        "action ship(c is a customer)",
        "action restart\n  requires every c in customer satisfies status of orders of c is shipped\n  status of orders of every customer becomes draft\n  items of orders of every customer becomes 1\n\naction ship(c is a customer)",
    );
    let verification = verify(&parse(&source).unwrap()).unwrap();
    let restarted = (0..verification.graph.state_count())
        .map(|state| verification.state(u32::try_from(state).unwrap()))
        .any(|state| {
            state.get("orders").map(Value::source).as_deref()
                == Some("{ann: {items: 1, status: draft}, ben: {items: 1, status: draft}}")
                && state.get("last").map(Value::source).as_deref()
                    != Some("{what: {items: 0, status: draft}, who: nobody}")
        });
    assert!(restarted);
}

#[test]
fn record_type_errors_name_the_member() {
    let missing = error(&ORDERS.replace(
        "orders of c becomes {status: shipped, items: items of orders of c}",
        "orders of c becomes {status: shipped}",
    ));
    assert_eq!(missing.message, "this record needs a value for items");
    let unknown = error(&ORDERS.replace(
        "requires status of orders of c is paid",
        "requires colour of orders of c is paid",
    ));
    assert_eq!(unknown.message, "orders has no member colour");
    let compared = error(&ORDERS.replace(
        "requires status of orders of c is paid",
        "requires orders of c is paid",
    ));
    assert_eq!(compared.kind, ErrorKind::Spec);
    assert!(
        compared.message.starts_with("cannot compare {status:"),
        "{compared}"
    );
    let partial = error(&ORDERS.replace(
        "  last is {who: nobody, what: {status: draft, items: 0}}\n",
        "",
    ));
    assert!(
        partial.message.contains("who of last has no initial value"),
        "{partial}"
    );
    let range = error(&ORDERS.replace(
        "items of orders of c becomes items of orders of c + 1",
        "items of orders of c becomes items of orders of c + 3",
    ));
    assert_eq!(range.kind, ErrorKind::Model);
    assert!(
        range
            .message
            .contains("items of orders of ann would become 3"),
        "{range}"
    );
}

#[test]
fn reduced_and_full_checks_agree_on_a_broken_record_model() {
    let broken = ORDERS.replace(
        "orders of c becomes {status: shipped, items: items of orders of c}",
        "orders of c becomes {status: shipped, items: 0}",
    );
    let full = error(&broken);
    let reduced = verify_with(&parse(&broken).unwrap(), Options::default()).unwrap_err();
    assert_eq!(full.kind, ErrorKind::Violation);
    assert_eq!(reduced.kind, full.kind);
    assert_eq!(reduced.line, full.line);
    assert_eq!(
        reduced.trace.map(|trace| trace.steps.len()),
        full.trace.map(|trace| trace.steps.len())
    );
}

/// An order book that keeps each order as a tuple, observed as nested maps.
/// It does not track payment days, so it never observes `paid_on`.
#[derive(Default)]
struct Book {
    orders: BTreeMap<String, (String, i64)>,
    last: Option<(String, String, i64)>,
}

impl Adapter for Book {
    fn call(&mut self, action: &str, args: &Args) -> Outcome {
        let customer = args.get("c").map(ToString::to_string).unwrap_or_default();
        let entry = self
            .orders
            .entry(customer.clone())
            .or_insert_with(|| ("draft".into(), 0));
        match action {
            "add" if entry.0 == "draft" && entry.1 < 2 => entry.1 += 1,
            "pay" if entry.0 == "draft" && entry.1 > 0 => {
                entry.0 = "paid".into();
                let items = entry.1;
                self.last = Some((customer, "paid".into(), items));
            }
            "ship" if entry.0 == "paid" => entry.0 = "shipped".into(),
            _ => return Outcome::Rejected,
        }
        Outcome::Accepted
    }

    fn observe(&self) -> Option<State> {
        let orders = self
            .orders
            .iter()
            .map(|(customer, (status, items))| {
                let order = if status == "draft" {
                    BTreeMap::from([("items".to_string(), Value::Integer(*items))])
                } else {
                    BTreeMap::from([
                        ("status".to_string(), Value::Symbol(status.clone())),
                        ("items".to_string(), Value::Integer(*items)),
                    ])
                };
                (customer.clone(), Value::Map(order))
            })
            .collect();
        let mut state = State::from([("orders".to_string(), Value::Map(orders))]);
        if let Some((who, status, items)) = &self.last {
            state.insert(
                "last".into(),
                Value::Map(BTreeMap::from([
                    ("who".to_string(), Value::Symbol(who.clone())),
                    (
                        "what".to_string(),
                        Value::Map(BTreeMap::from([
                            ("status".to_string(), Value::Symbol(status.clone())),
                            ("items".to_string(), Value::Integer(*items)),
                        ])),
                    ),
                ])),
            );
        }
        Some(state)
    }
}

#[test]
fn an_implementation_with_structs_conforms_and_a_wrong_member_is_caught() {
    let report = check(ORDERS, &PlanOptions::default(), Book::default).unwrap();
    assert!(report.tests > 0);

    #[derive(Default)]
    struct Forgetful(Book);
    impl Adapter for Forgetful {
        fn call(&mut self, action: &str, args: &Args) -> Outcome {
            let outcome = self.0.call(action, args);
            if action == "ship" {
                for order in self.0.orders.values_mut() {
                    if order.0 == "shipped" {
                        order.1 = 0;
                    }
                }
            }
            outcome
        }
        fn observe(&self) -> Option<State> {
            self.0.observe()
        }
    }
    let error = check(ORDERS, &PlanOptions::default(), Forgetful::default).unwrap_err();
    assert!(error.message.contains("orders is {"), "{error}");
}

const LEASE: &str = include_str!("../examples/lease.zin");

#[test]
fn optional_values_have_one_encoding_under_symmetry() {
    let full = verify(&parse(LEASE).unwrap()).unwrap();
    assert_eq!(full.graph.state_count(), 10);
    let reduced = verify_with(&parse(LEASE).unwrap(), Options::default()).unwrap();
    assert_eq!(reduced.graph.state_count(), 4);
    assert_eq!(full.state(0).get("lease"), Some(&Value::Nothing));
    let plan = Plan::build(&full, &PlanOptions::default()).unwrap();
    let json = plan.to_json(&full);
    assert_eq!(json.get("zin"), Some(&Json::str("plan/2")));
    let Some(Json::Array(states)) = json.get("states") else {
        panic!("no states")
    };
    assert_eq!(states[0].to_compact(), r#"{"last":null,"lease":null}"#);
}

#[test]
fn reading_nothing_is_a_fault_unless_guarded() {
    let unguarded = error(&LEASE.replace(
        "  lease is not nothing and left of lease is 1",
        "  left of lease is 1",
    ));
    assert_eq!(unguarded.kind, ErrorKind::Model);
    assert!(
        unguarded
            .message
            .starts_with("lease is nothing while evaluating"),
        "{unguarded}"
    );
    let compared = error(&LEASE.replace("  last is nothing\n", "  last is 0\n"));
    assert!(compared.message.contains("assigns a number"), "{compared}");
}

#[test]
fn a_value_compares_with_an_optional_without_unwrapping_it() {
    let source = r#"system "Ask"
state
  asked is an integer from 1 to 3 or nothing
initially
  asked is nothing
always
  asked is 2 implies asked is not nothing
sometimes
  asked is 2
never
  asked is 0
action ask(n is an integer from 1 to 3)
  requires asked is nothing
  asked becomes n
action answer
  requires asked is not nothing
  asked becomes if asked > 2 then nothing else asked + 1
"#;
    let verification = verify(&parse(source).unwrap()).unwrap();
    assert_eq!(verification.graph.state_count(), 4);
}

#[test]
fn an_implementation_with_options_conforms() {
    #[derive(Default)]
    struct Lease {
        held: Option<(String, i64)>,
        last: Option<String>,
    }
    impl Adapter for Lease {
        fn call(&mut self, action: &str, args: &Args) -> Outcome {
            let worker = args.get("w").map(ToString::to_string);
            match (action, &mut self.held) {
                ("acquire", None) => {
                    self.held = worker.clone().map(|w| (w, 2));
                    self.last = worker;
                }
                ("tick", Some((_, 1))) => self.held = None,
                ("tick", Some((_, left))) => *left -= 1,
                ("release", Some((holder, _))) if Some(holder.clone()) == worker => {
                    self.held = None;
                }
                _ => return Outcome::Rejected,
            }
            Outcome::Accepted
        }
        fn observe(&self) -> Option<State> {
            let optional = |value: Option<Value>| value.unwrap_or(Value::Nothing);
            Some(State::from([
                (
                    "lease".to_string(),
                    optional(self.held.as_ref().map(|(holder, left)| {
                        Value::Map(BTreeMap::from([
                            ("holder".to_string(), Value::Symbol(holder.clone())),
                            ("left".to_string(), Value::Integer(*left)),
                        ]))
                    })),
                ),
                (
                    "last".to_string(),
                    optional(self.last.clone().map(Value::Symbol)),
                ),
            ]))
        }
    }
    let report = check(LEASE, &PlanOptions::default(), Lease::default).unwrap();
    assert!(report.tests > 0);
}
