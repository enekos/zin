use zin::{Error, parse, verify};

fn failure(source: &str) -> Error {
    parse(source).and_then(|spec| verify(&spec)).unwrap_err()
}

fn at(error: &Error) -> (usize, usize, usize) {
    let span = error.source_span().expect("located error");
    (span.line, span.column, span.end_column)
}

const WORKERS: &str = "system \"Workers\"
types
  worker is one of ann, ben
state
  busy is a map from worker to a boolean
initially
  busy of every worker is false
";

#[test]
fn an_unknown_name_points_at_its_own_occurrence() {
    let source = format!(
        "{WORKERS}action start(v is a worker)\n  requires (some w in worker satisfies busy of w) or busy of w\n  busy of v becomes true\n"
    );
    let error = failure(&source);
    assert_eq!(error.code(), "name.unknown");
    assert!(error.message.contains("unknown name w"), "{error}");
    // `w` also appears as the bound variable and inside the quantifier.
    assert_eq!(at(&error), (9, 62, 63));
    assert_eq!(error.line, Some(9));
}

#[test]
fn an_unknown_map_points_at_the_map_name() {
    let error = failure(&format!(
        "{WORKERS}action start(w is a worker)\n  requires not busy of w and not idle of w\n"
    ));
    assert_eq!(error.code(), "name.unknown");
    assert_eq!(at(&error), (9, 34, 38));
}

#[test]
fn type_mismatches_cover_the_statement() {
    let error = failure(&format!(
        "{WORKERS}action start(w is a worker)\n  requires busy of w and 1\n"
    ));
    assert_eq!(error.code(), "type.mismatch");
    assert_eq!(at(&error), (9, 12, 27));
    let error = failure(&format!(
        "{WORKERS}action start(w is a worker)\n  busy of w becomes 1   # wrong kind\n"
    ));
    assert_eq!(error.code(), "type.mismatch");
    assert_eq!(at(&error), (9, 3, 7));
}

#[test]
fn syntax_errors_point_at_the_offending_token() {
    let error = failure(&format!(
        "{WORKERS}action start(w is a worker)\n  requires busy of w and )\n"
    ));
    assert_eq!(error.code(), "spec");
    assert_eq!(at(&error), (9, 26, 27));
    let error =
        failure("system \"T\"\nstate\n  n is an integer from 0 to 1\ninitially\n  n is \"open\n");
    assert!(error.message.contains("unterminated"), "{error}");
    assert_eq!(at(&error), (5, 8, 13));
}

#[test]
fn a_statement_without_a_token_is_highlighted_whole() {
    let error = failure(
        "system \"T\"\nstate\n  n is an integer from 0 to 1\ninitially\n  n is 0\nexplore at most 0 states   # too few\n",
    );
    assert_eq!(at(&error), (6, 1, 25));
}

#[test]
fn conflicting_declarations_report_both_sites() {
    let error = failure(
        "system \"A\"\nstate\n  on is a boolean\ninitially\n  on is false\nsystem \"B\"\nstate\n  x is a boolean\n  on is a boolean\ninitially\n  x is false\n  on is false\n",
    );
    assert_eq!(error.code(), "component.field_conflict");
    assert_eq!(at(&error), (9, 3, 5));
    let related = error.related_span().unwrap();
    assert_eq!(
        (related.line, related.column, related.end_column),
        (3, 3, 5)
    );
    assert!(
        error.to_string().ends_with("(first declared on line 3)"),
        "{error}"
    );
}
