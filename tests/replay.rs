use zin::{ErrorKind, audit, check_trace_reader, parse, verify};

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

#[test]
fn trace_streaming_checks_exact_byte_limit_and_line_numbers() {
    let model = verify(&parse(SWITCH).unwrap()).unwrap().model;
    let good = b"{\"action\":\"start\"}\n";
    let read = |bytes: &[u8], limit| {
        check_trace_reader(&model, std::io::BufReader::with_capacity(2, bytes), limit)
    };
    assert_eq!(read(good, good.len() as u64).unwrap().events, 1);
    assert_eq!(
        read(good, good.len() as u64 - 1).unwrap_err().kind,
        ErrorKind::Usage
    );
    let err = read(b"\n{\"action\":\"stop\"}\n", 200).unwrap_err();
    assert!(err.message.contains("line 2"), "{err}");
}

#[test]
fn audit_gives_a_shortest_distinguishing_example_for_every_survivor() {
    let report = audit(&parse(SWITCH).unwrap()).unwrap();
    let survivors = report.survivors();
    assert!(!survivors.is_empty());
    for mutant in survivors {
        let witness = mutant.witness.as_ref().unwrap();
        assert_ne!(witness.original, witness.mutated);
        assert!(witness.prefix.len() <= 1);
    }
    let dropped = report
        .mutants
        .iter()
        .find(|m| m.description == "action start: drop 'requires not active'")
        .unwrap();
    let witness = dropped.witness.as_ref().unwrap();
    assert_eq!(witness.prefix, ["start"]);
    assert_eq!(witness.call, "start");
    assert!(witness.original.is_none() && witness.mutated.is_some());
}
