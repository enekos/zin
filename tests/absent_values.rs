//! `or else` defaults for reads that can find no value.

use zin::ast::{BinaryOperator, Expr};
use zin::{ErrorKind, Options, parse, verify, verify_with};

const HEADER: &str = "system \"Reads\"
types
  worker is one of ann, ben
  message is one of ping, pong
state
  lease is a record with (holder is a worker, left is an integer from 0 to 2) or nothing
  last is a worker or nothing
  queue is a list of at most 2 message
  job is one of idle, running with (attempt is an integer from 1 to 2), failed
  seen is an integer from 0 to 3
  asked is an integer from 1 to 3 or nothing
initially
  lease is nothing
  last is nothing
  queue is []
  job is idle
  seen is 0
  asked is nothing
action acquire(w is a worker)
  requires lease is nothing
  lease becomes {holder: w, left: 2}
  last becomes w
action send(m is a message)
  requires length of queue < 2
  queue becomes queue with m
action start
  requires job is idle
  job becomes running with {attempt: 1}
";

fn spec(rest: &str) -> String {
    format!("{HEADER}{rest}")
}

fn error(source: &str) -> zin::Error {
    verify(&parse(source).unwrap()).unwrap_err()
}

#[test]
fn or_else_binds_tighter_than_arithmetic() {
    let spec = parse(&spec(
        "action tick\n  seen becomes left of lease or else 0 + 1\n",
    ))
    .unwrap();
    let value = &spec.actions.last().unwrap().assignments[0].value;
    let Expr::Binary {
        operator: BinaryOperator::Add,
        left,
        ..
    } = value
    else {
        panic!("{value:?}");
    };
    let Expr::OrElse { value, default } = left.as_ref() else {
        panic!("{left:?}");
    };
    assert!(matches!(value.as_ref(), Expr::Index { map, .. } if map == "left"));
    assert!(matches!(default.as_ref(), Expr::Literal(_)));
}

#[test]
fn or_else_reads_each_kind_of_absent_value() {
    let source = spec(
        "action tick
  seen becomes left of lease or else 0 + 1
action drop
  queue becomes rest of queue or else []
action fail
  job becomes failed
  seen becomes attempt of job or else 3
always (first of queue or else ping) is ping or queue is not []
always (last or else ann) is ann or last is ben
always (holder of lease or else ann) is ann or lease is not nothing
",
    );
    let verification = verify(&parse(&source).unwrap()).unwrap();
    let seen: Vec<i64> = (0..verification.graph.state_count())
        .filter_map(|state| {
            match verification
                .state(u32::try_from(state).unwrap())
                .get("seen")
            {
                Some(zin::Value::Integer(seen)) => Some(*seen),
                _ => None,
            }
        })
        .collect();
    assert!(seen.contains(&1) && seen.contains(&3), "{seen:?}");
}

#[test]
fn without_or_else_the_same_reads_fault() {
    for (read, fault) in [
        ("seen becomes left of lease", "lease is nothing"),
        ("queue becomes rest of queue", "queue is empty"),
        (
            "seen becomes attempt of job",
            "job is not running, so it has no attempt",
        ),
    ] {
        let error = error(&spec(&format!("action tick\n  {read}\n")));
        assert_eq!(error.kind, ErrorKind::Model, "{error}");
        assert!(error.message.contains(fault), "{error}");
    }
}

#[test]
fn the_default_is_evaluated_only_when_used() {
    let present = spec("action tick\n  seen becomes left of lease or else (2 / seen)\n").replace(
        "lease is nothing\n  last",
        "lease is {holder: ann, left: 1}\n  last",
    );
    let present = present.replace("requires lease is nothing\n", "requires false\n");
    verify(&parse(&present).unwrap()).unwrap();
    let absent = spec("action tick\n  seen becomes left of lease or else (2 / seen)\n");
    let error = error(&absent);
    assert!(error.message.contains("division by zero"), "{error}");
}

#[test]
fn or_else_only_catches_absent_values() {
    let error = error(&spec(
        "action tick\n  seen becomes (left of lease + 2) or else 0\n",
    ));
    assert!(error.message.contains("seen would become"), "{error}");
}

#[test]
fn or_else_needs_a_read_that_can_be_absent_and_a_default_of_its_type() {
    let present = error(&spec("action tick\n  seen becomes seen or else 0\n"));
    assert_eq!(present.kind, ErrorKind::Spec);
    assert!(
        present.message.contains("seen is always there"),
        "{present}"
    );
    let mismatch = error(&spec(
        "action tick\n  seen becomes left of lease or else ann\n",
    ));
    assert_eq!(mismatch.code(), "type.mismatch", "{mismatch}");
    assert!(
        mismatch
            .message
            .contains("the default after 'or else' must be a number"),
        "{mismatch}"
    );
}

#[test]
fn or_else_keeps_symmetry_reduction_on() {
    let model = |read: &str| {
        format!(
            "system \"Leases\"
types
  interchangeable worker is one of ann, ben, cid
state
  left is a map from worker to an integer from 1 to 2 or nothing
initially
  left of every worker is nothing
action take(w is a worker)
  requires left of w is nothing
  left of w becomes 2
action tick(w is a worker)
  requires left of w is 2
  left of w becomes 1
always some w in worker satisfies {read} >= 0
"
        )
    };
    let unguarded = verify_with(&parse(&model("left of w")).unwrap(), Options::default());
    let unguarded = unguarded.unwrap_err();
    assert_eq!(unguarded.kind, ErrorKind::Model, "{unguarded}");
    let guarded = verify_with(
        &parse(&model("(left of w or else 0)")).unwrap(),
        Options::default(),
    )
    .unwrap();
    assert!(guarded.reduced, "{:?}", guarded.notes);
}
