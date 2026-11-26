//! `or else` defaults for reads that can find no value, and the notes that
//! point at such reads when nothing guards them.

use zin::ast::{BinaryOperator, Expr};
use zin::{ErrorKind, Json, Options, parse, verify, verify_with};

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

/// The line and column of each unguarded read, and what it reads.
fn unguarded(source: &str) -> Vec<(usize, usize, String)> {
    verify(&parse(source).unwrap())
        .unwrap()
        .unguarded
        .into_iter()
        .map(|read| {
            let span = read.span.unwrap();
            (span.line, span.column, read.read)
        })
        .collect()
}

fn line_of(source: &str, text: &str) -> usize {
    source.lines().position(|line| line.contains(text)).unwrap() + 1
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
    assert!(
        verification.unguarded.is_empty(),
        "{:?}",
        verification.notes
    );
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

#[test]
fn guarded_reads_need_no_note() {
    let source = spec(
        "action a1
  requires lease is not nothing
  seen becomes left of lease
action a2
  requires length of queue > 0
  queue becomes rest of queue
action a3
  requires 0 < length of queue and not (length of queue is 0)
  queue becomes [last of queue]
action a4
  requires job is running and attempt of job is 2
  job becomes failed
action a5
  requires not (job is idle) and not (job is failed) and attempt of job is 1
  job becomes idle
action a6(w is a worker)
  requires last is w
  seen becomes if queue is [] then 0 else (if first of queue is ping then 1 else 2)
action a7
  requires queue has ping or length of queue >= 1
  seen becomes 1
action a8
  requires job is running with {attempt: 1}
  seen becomes attempt of job
always job is idle or job is failed or attempt of job > 0
always queue has ping implies first of queue is ping or true
always lease is nothing or holder of lease is ann or holder of lease is ben
always not (queue is []) implies last of queue is ping or last of queue is pong
always if last is nothing then true else last is ann or last is ben
always asked is nothing or asked > 0
",
    );
    assert_eq!(unguarded(&source), Vec::new());
}

#[test]
fn unguarded_reads_each_get_one_note_at_the_read() {
    let source = spec(
        "action u1
  requires seen is 3
  seen becomes left of lease
action u2
  requires seen is 3 and last is ann
  queue becomes [first of queue]
action u3
  requires job is not idle and seen is 3
  seen becomes attempt of job
action u4(w is a worker)
  requires lease is not nothing and last is w
  seen becomes number of v in worker where last is v and seen is left of lease
action u5
  requires asked is seen
  seen becomes asked - 1
always job is idle or attempt of job > 0 or job is failed
always seen < 4 or asked > 0
",
    );
    let at = |text: &str, read: &str| {
        let line = line_of(&source, text);
        let column = source.lines().nth(line - 1).unwrap().find(read).unwrap() + 1;
        (line, column, read.to_string())
    };
    let expected = vec![
        at("seen becomes left of lease", "left of lease"),
        at("queue becomes [first of queue]", "first of queue"),
        at("seen becomes attempt of job", "attempt of job"),
        at("seen becomes asked - 1", "asked"),
        at("always job is idle or attempt", "attempt of job"),
        at("always seen < 4 or asked > 0", "asked"),
    ];
    let found = unguarded(&source);
    assert_eq!(found, expected, "{found:?}");
}

#[test]
fn notes_name_the_guard_and_the_default() {
    let verification = verify(
        &parse(&spec(
            "action u\n  requires seen is 3\n  seen becomes left of lease\n",
        ))
        .unwrap(),
    )
    .unwrap();
    let note = verification.notes.last().unwrap();
    assert!(
        note.ends_with("'left of lease' faults if lease is nothing; no reachable state gets there, but nothing guards it: write 'lease is not nothing and ...' before it, or 'left of lease or else ...'"),
        "{note}"
    );
    assert!(
        note.starts_with(&format!("line {}: ", HEADER.lines().count() + 3)),
        "{note}"
    );
}

#[test]
fn check_json_lists_unguarded_reads() {
    let source = spec("action u\n  requires seen is 3\n  seen becomes left of lease\n");
    let path = std::env::temp_dir().join(format!("zin-unguarded-{}.zin", std::process::id()));
    std::fs::write(&path, &source).unwrap();
    let args = ["check", "--format", "json", path.to_str().unwrap()].map(String::from);
    let output = zin::cli::run(&args);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(output.code, 0, "{}", output.stderr);
    let parsed = Json::parse(output.stdout.trim()).unwrap();
    let Some(Json::Array(results)) = parsed.get("results") else {
        panic!("{}", output.stdout);
    };
    let Some(Json::Array(reads)) = results[0].get("unguarded_reads") else {
        panic!("{}", output.stdout);
    };
    assert_eq!(reads.len(), 1, "{}", output.stdout);
    let read = &reads[0];
    assert_eq!(
        read.get("code").and_then(Json::as_str),
        Some("read.unguarded")
    );
    assert_eq!(
        read.get("read").and_then(Json::as_str),
        Some("left of lease")
    );
    assert_eq!(read.get("subject").and_then(Json::as_str), Some("lease"));
    let span = read.get("span").unwrap();
    assert_eq!(
        span.get("line"),
        Some(&Json::Int(HEADER.lines().count() as i64 + 3))
    );
}
