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
fn plans_nest_record_values() {
    let verification = verify(&parse(ORDERS).unwrap()).unwrap();
    let plan = Plan::build(&verification, &PlanOptions::default()).unwrap();
    let json = plan.to_json(&verification);
    assert_eq!(json.get("zin"), Some(&Json::str("plan/1")));
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
/// It does not track payment days, so it never observes `paid_on`; the
/// fields it does observe are complete.
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
        let orders = ["ann", "ben"]
            .into_iter()
            .map(|customer| {
                let (status, items) = self
                    .orders
                    .get(customer)
                    .cloned()
                    .unwrap_or_else(|| ("draft".into(), 0));
                let order = BTreeMap::from([
                    ("status".to_string(), Value::Symbol(status)),
                    ("items".to_string(), Value::Integer(items)),
                ]);
                (customer.to_string(), Value::Map(order))
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

const ROOMS: &str = include_str!("../examples/rooms.zin");

#[test]
fn sets_check_with_and_without_symmetry() {
    let full = verify(&parse(ROOMS).unwrap()).unwrap();
    assert_eq!(full.graph.state_count(), 34);
    let reduced = verify_with(&parse(ROOMS).unwrap(), Options::default()).unwrap();
    assert_eq!(reduced.graph.state_count(), 13);
    let plan = Plan::build(&full, &PlanOptions::default()).unwrap();
    let json = plan.to_json(&full);
    assert!(
        json.get("fields")
            .unwrap()
            .to_compact()
            .contains(r#"{"kind":"set","values":["ann","ben","cid"]}"#)
    );
    let broken = ROOMS.replace(
        "  inside becomes inside without w\n",
        "  inside becomes inside without w\n  visited becomes visited without w\n",
    );
    let full = error(&broken);
    let reduced = verify_with(&parse(&broken).unwrap(), Options::default()).unwrap_err();
    assert_eq!(full.kind, ErrorKind::Violation);
    assert_eq!((reduced.kind, reduced.line), (full.kind, full.line));
}

#[test]
fn set_type_errors() {
    let has = error(&ROOMS.replace("requires inside has w", "requires open has w"));
    assert_eq!(
        has.message,
        "'has' needs a set on its left, found a true/false value"
    );
    let element = error(&ROOMS.replace("requires inside has w", "requires inside has 1"));
    assert!(
        element
            .message
            .starts_with("this set holds one of ann, ben, cid, not a number"),
        "{element}"
    );
    let literal = error(&ROOMS.replace("not open and inside is not []", "[] is []"));
    assert!(
        literal.message.starts_with("a [...] literal needs a set"),
        "{literal}"
    );
    let property = error(&ROOMS.replace("size of inside <= 2", "length of inside <= 2"));
    assert!(
        property.message.starts_with("a set has no length"),
        "{property}"
    );
}

#[derive(Default)]
struct Room {
    inside: Vec<String>,
    visited: Vec<String>,
    open: bool,
    started: bool,
}

impl Adapter for Room {
    fn call(&mut self, action: &str, args: &Args) -> Outcome {
        if !self.started {
            self.open = true;
            self.started = true;
        }
        let who = args.get("w").map(ToString::to_string).unwrap_or_default();
        match action {
            "enter" if self.open && !self.inside.contains(&who) && self.inside.len() < 2 => {
                self.inside.insert(0, who.clone());
                if !self.visited.contains(&who) {
                    self.visited.insert(0, who);
                }
            }
            "leave" if self.inside.contains(&who) => self.inside.retain(|w| *w != who),
            "close" if self.open && self.inside.is_empty() => self.open = false,
            _ => return Outcome::Rejected,
        }
        Outcome::Accepted
    }

    fn observe(&self) -> Option<State> {
        let list =
            |names: &[String]| Value::List(names.iter().cloned().map(Value::Symbol).collect());
        Some(State::from([
            ("inside".to_string(), list(&self.inside)),
            ("visited".to_string(), list(&self.visited)),
        ]))
    }
}

#[test]
fn a_set_observed_in_another_order_conforms() {
    let report = check(ROOMS, &PlanOptions::default(), Room::default).unwrap();
    assert!(report.tests > 0);
    let model = verify(&parse(ROOMS).unwrap()).unwrap().model;
    let events = concat!(
        r#"{"action":"enter","args":["ben"]}"#,
        "\n",
        r#"{"action":"enter","args":["ann"],"state":{"inside":["ben","ann"],"visited":["ben","ann"]}}"#,
        "\n"
    );
    zin::check_trace(&model, events).unwrap();
    let wrong = events.replace(r#""visited":["ben","ann"]"#, r#""visited":["ben"]"#);
    assert_eq!(
        zin::check_trace(&model, &wrong).unwrap_err().kind,
        ErrorKind::Violation
    );
}

const CHANNEL: &str = include_str!("../examples/channel.zin");
const RECENCY: &str = include_str!("../examples/recency.zin");

#[test]
fn lists_keep_order_and_have_one_encoding() {
    let channel = verify(&parse(CHANNEL).unwrap()).unwrap();
    assert_eq!(channel.graph.state_count(), 41);
    let full = verify(&parse(RECENCY).unwrap()).unwrap();
    assert_eq!(full.graph.state_count(), 10);
    let reduced = verify_with(&parse(RECENCY).unwrap(), Options::default()).unwrap();
    assert_eq!(reduced.graph.state_count(), 3);
    let plan = Plan::build(&full, &PlanOptions::default()).unwrap();
    let json = plan.to_json(&full);
    assert!(json.get("fields").unwrap().to_compact().contains(
        r#"{"kind":"list","capacity":2,"element":{"kind":"enum","values":["a","b","c"]}}"#
    ));
}

#[test]
fn list_faults_and_type_errors() {
    let full = error(&CHANNEL.replace(
        "requires sent < 3 and length of queue < 2",
        "requires sent < 3",
    ));
    assert_eq!(full.kind, ErrorKind::Model);
    assert!(
        full.message.ends_with("send(ping): queue is full"),
        "{full}"
    );
    let empty = error(&CHANNEL.replace("  requires queue is not []\n", ""));
    assert!(
        empty.message.ends_with("deliver: queue is empty"),
        "{empty}"
    );
    let literal = error(&CHANNEL.replace("  queue is []\n", "  queue is [ping, pong, ping]\n"));
    assert!(
        literal
            .message
            .starts_with("this list holds at most 2 items, and 3 are given"),
        "{literal}"
    );
    let item = error(&CHANNEL.replace("queue becomes queue with m", "queue becomes queue with 1"));
    assert!(
        item.message
            .starts_with("this list holds ping or pong, not a number"),
        "{item}"
    );
    let property = error(&CHANNEL.replace("length of log is sent", "size of log is sent"));
    assert!(
        property.message.starts_with("a list has no size"),
        "{property}"
    );
}

#[test]
fn removing_from_a_list_keeps_the_others_in_order() {
    let source = r#"system "Remove"
types
  item is one of a, b
state
  l is a list of at most 4 item
initially
  l is [a, b, a, b]
action drop_a
  requires l has a
  l becomes l without a
action drop_b
  requires l has b
  l becomes l without b
scenario "Drop a then b"
  drop_a
  expect l is [b, b] and length of l is 2
  drop_b
  expect l is []
  reject drop_a
scenario "Drop b"
  drop_b
  expect l is [a, a] and first of l is a
"#;
    verify(&parse(source).unwrap()).unwrap();
}

#[test]
fn an_implementation_with_a_vec_conforms_and_a_stack_does_not() {
    #[derive(Default)]
    struct Queue {
        queue: Vec<String>,
        received: Vec<String>,
        log: Vec<String>,
        lifo: bool,
    }
    impl Adapter for Queue {
        fn call(&mut self, action: &str, args: &Args) -> Outcome {
            match action {
                "send" if self.log.len() < 3 && self.queue.len() < 2 => {
                    let message = args.get("m").map(ToString::to_string).unwrap_or_default();
                    self.queue.push(message.clone());
                    self.log.push(message);
                }
                "deliver" if !self.queue.is_empty() => {
                    let message = if self.lifo {
                        self.queue.pop().unwrap_or_default()
                    } else {
                        self.queue.remove(0)
                    };
                    self.received.push(message);
                }
                _ => return Outcome::Rejected,
            }
            Outcome::Accepted
        }
        fn observe(&self) -> Option<State> {
            let list =
                |items: &[String]| Value::List(items.iter().cloned().map(Value::Symbol).collect());
            Some(State::from([
                ("queue".to_string(), list(&self.queue)),
                ("received".to_string(), list(&self.received)),
            ]))
        }
    }
    check(CHANNEL, &PlanOptions::default(), Queue::default).unwrap();
    let error = check(CHANNEL, &PlanOptions::default(), || Queue {
        lifo: true,
        ..Queue::default()
    })
    .unwrap_err();
    assert!(
        error
            .message
            .contains("queue is [ping], but the specification says [pong]"),
        "{error}"
    );
}

const RETRY: &str = include_str!("../examples/retry.zin");

#[test]
fn enumeration_cases_carry_members_only_while_they_hold() {
    let verification = verify(&parse(RETRY).unwrap()).unwrap();
    assert_eq!(verification.graph.state_count(), 11);
    let plan = Plan::build(&verification, &PlanOptions::default()).unwrap();
    let json = plan.to_json(&verification);
    let Some(Json::Array(states)) = json.get("states") else {
        panic!("no states")
    };
    assert_eq!(
        states[1].to_compact(),
        r#"{"job":{"running":{"attempt":1}},"tries":1}"#
    );
    assert_eq!(states[0].to_compact(), r#"{"job":"queued","tries":0}"#);
    assert!(
        json.get("fields")
            .unwrap()
            .to_compact()
            .contains(r#""kind":"variant""#)
    );
}

#[test]
fn case_errors_and_faults() {
    let bare = error(&RETRY.replace("  job becomes succeeded", "  job becomes running"));
    assert_eq!(
        bare.message,
        "running carries members; write 'running with {attempt: ...}'"
    );
    let read = error(&RETRY.replace(
        "requires job is running and attempt of job is 3",
        "requires attempt of job is 3",
    ));
    assert_eq!(read.kind, ErrorKind::Model);
    assert!(
        read.message
            .contains("job is not running, so it has no attempt"),
        "{read}"
    );
    let member = error(&RETRY.replace(
        "failed with {reason: crashed} else",
        "failed with {cause: crashed} else",
    ));
    assert!(
        member
            .message
            .starts_with("this record has no member cause"),
        "{member}"
    );
}

#[test]
fn an_implementation_with_an_enum_conforms() {
    #[derive(Default)]
    enum Job {
        #[default]
        Queued,
        Running(i64),
        Succeeded,
        Failed(&'static str),
    }
    #[derive(Default)]
    struct Runner {
        job: Job,
        tries: i64,
    }
    impl Adapter for Runner {
        fn call(&mut self, action: &str, _: &Args) -> Outcome {
            self.job = match (action, &self.job) {
                ("start", Job::Queued) => {
                    self.tries += 1;
                    Job::Running(self.tries)
                }
                ("succeed", Job::Running(_)) => Job::Succeeded,
                ("crash", Job::Running(3)) => Job::Failed("crashed"),
                ("crash", Job::Running(_)) => Job::Queued,
                ("time_out", Job::Running(3)) => Job::Failed("timed_out"),
                _ => return Outcome::Rejected,
            };
            Outcome::Accepted
        }
        fn observe(&self) -> Option<State> {
            let tagged = |tag: &str, member: &str, value: Value| Value::Tagged {
                tag: tag.into(),
                members: BTreeMap::from([(member.to_string(), value)]),
            };
            let job = match &self.job {
                Job::Queued => Value::Symbol("queued".into()),
                Job::Running(attempt) => tagged("running", "attempt", Value::Integer(*attempt)),
                Job::Succeeded => Value::Symbol("succeeded".into()),
                Job::Failed(reason) => tagged("failed", "reason", Value::Symbol((*reason).into())),
            };
            Some(State::from([
                ("job".to_string(), job),
                ("tries".to_string(), Value::Integer(self.tries)),
            ]))
        }
    }
    check(RETRY, &PlanOptions::default(), Runner::default).unwrap();
}

#[test]
fn map_literal_entries_of_different_types_share_one_layout() {
    let source = "\
system \"Mixed\"
types
  key is one of ka, kb, kc
  phase is one of idle, run with (n is an integer from 0 to 3)
state
  level is a map from key to an integer from 0 to 3 or nothing
  task is a map from key to a phase
  hold is a map from key to a record with (a is a boolean or nothing, b is an integer from 0 to 3)
initially
  level is {ka: nothing, kb: 2, kc: nothing}
  task is {ka: idle, kb: run with {n: 2}, kc: idle}
  hold of every key is {a: nothing, b: 0}
always level is {ka: nothing, kb: 2, kc: nothing} or level is {ka: 1, kb: nothing, kc: 1}
always task is {ka: idle, kb: run with {n: 2}, kc: idle}
action set
  level becomes {ka: 1, kb: nothing, kc: 1}
  hold becomes {ka: {a: nothing, b: 1}, kb: {a: true, b: 2}, kc: {a: nothing, b: 3}}
scenario \"set\"
  expect level of kb is 2 and level of ka is nothing
  expect task of kb is run with {n: 2}
  set
  expect level of ka is 1 and level of kb is nothing and level of kc is 1
  expect hold of kb is {a: true, b: 2} and hold of ka is {a: nothing, b: 1}
";
    let verification = verify(&parse(source).unwrap()).unwrap();
    assert_eq!(verification.graph.state_count(), 2);
}
