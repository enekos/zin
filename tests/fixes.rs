use zin::{Error, parse, parse_file, verify};

fn failure(source: &str) -> Error {
    parse(source).and_then(|spec| verify(&spec)).unwrap_err()
}

/// The replacement and replaced columns of each fix.
fn fixes(error: &Error) -> Vec<(String, usize, usize, usize)> {
    error
        .fixes()
        .map(|fix| {
            (
                fix.replacement.clone(),
                fix.span.line,
                fix.span.column,
                fix.span.end_column,
            )
        })
        .collect()
}

fn fix(
    replacement: &str,
    line: usize,
    column: usize,
    end: usize,
) -> Vec<(String, usize, usize, usize)> {
    vec![(replacement.to_owned(), line, column, end)]
}

const ORDERS: &str = "system \"Orders\"
types
  phase is one of pending, paid, shipped
  customer is one of ann, ben
  order is a record with (status is a phase, items is an integer from 0 to 2)
state
  status is a phase
  pendnig is an integer from 0 to 2
  orders is a map from customer to order
  seen is a set of customer
initially
  status is pending
  pendnig is 0
  orders of every customer is {status: pending, items: 0}
  seen is []
";

#[test]
fn unknown_names_suggest_the_closest_declared_name() {
    let cases = [
        // An enum value where the field's type wants one.
        (
            "action pay\n  requires status is pendng\n",
            fix("pending", 17, 22, 28),
        ),
        // A state field.
        (
            "action pay\n  requires stauts is pending\n",
            fix("status", 17, 12, 18),
        ),
        // A parameter.
        (
            "action pay(c is a customer)\n  requires items of orders of cc is 1\n",
            fix("c", 17, 31, 33),
        ),
        // A map.
        (
            "action pay(c is a customer)\n  requires items of oders of c is 1\n",
            fix("orders", 17, 21, 26),
        ),
        // A record member.
        (
            "action pay(c is a customer)\n  requires stats of orders of c is paid\n",
            fix("status", 17, 12, 17),
        ),
        // The type a quantifier ranges over.
        (
            "always every c in custmer satisfies items of orders of c <= 2\n",
            fix("customer", 16, 19, 26),
        ),
        // A quoted enum value.
        (
            "action pay\n  requires status is \"paied\"\n",
            fix("\"paid\"", 17, 22, 29),
        ),
        // A record literal's member.
        (
            "action pay(c is a customer)\n  orders of c becomes {stauts: paid, items: 0}\n",
            fix("status", 17, 24, 30),
        ),
        // A map literal's key.
        (
            "always orders is {ann: {status: pending, items: 0}, bn: {status: pending, items: 0}}\n",
            fix("ben", 16, 53, 55),
        ),
        // A scenario's action and parameter.
        (
            "action pay\n  status becomes paid\nscenario \"S\"\n  ppay\n",
            fix("pay", 19, 3, 7),
        ),
        (
            "action pay(c is a customer)\n  status becomes paid\nscenario \"S\"\n  pay with cc = ann\n",
            fix("c", 19, 12, 14),
        ),
    ];
    for (tail, expected) in cases {
        let error = failure(&format!("{ORDERS}{tail}"));
        assert_eq!(error.code(), "name.unknown", "{tail}: {error}");
        assert_eq!(fixes(&error), expected, "{tail}: {}", error.report());
    }
}

#[test]
fn unknown_type_names_in_declarations_suggest_declared_types() {
    let source = "system \"T\"\ntypes\n  worker is one of ann, ben\nstate\n  busy is a map from wroker to a boolean\ninitially\n  busy of every worker is false\n";
    let error = failure(source);
    assert_eq!(error.code(), "name.unknown");
    assert_eq!(fixes(&error), fix("worker", 5, 22, 28));
    let source = "system \"T\"\ntypes\n  worker is one of ann, ben\nstate\n  on is a set of wroker\ninitially\n  on is []\n";
    assert_eq!(fixes(&failure(source)), fix("worker", 5, 18, 24));
    let source = "system \"T\"\ntypes\n  worker is one of ann, ben\nstate\n  who is a wroker\ninitially\n  who is ann\n";
    assert_eq!(fixes(&failure(source)), fix("worker", 5, 12, 18));
}

#[test]
fn suggestions_fit_the_type_the_context_needs() {
    // `pending` and the integer field `pendnig` are both one edit from `pendng`.
    let error = failure(&format!(
        "{ORDERS}action pay\n  requires status is pendng\n"
    ));
    assert_eq!(fixes(&error), fix("pending", 17, 22, 28));
    let error = failure(&format!("{ORDERS}action pay\n  requires 1 is pendng\n"));
    assert_eq!(fixes(&error), fix("pendnig", 17, 17, 23));
}

#[test]
fn equally_close_names_are_all_offered_in_order() {
    let error = failure(
        "system \"T\"\nstate\n  pad is a boolean\n  pod is a boolean\ninitially\n  pad is false\n  pod is false\nalways pid or true\n",
    );
    assert_eq!(
        fixes(&error),
        [("pad".to_owned(), 8, 8, 11), ("pod".to_owned(), 8, 8, 11)]
    );
}

#[test]
fn nothing_is_suggested_when_no_name_is_close() {
    let error = failure(&format!(
        "{ORDERS}action pay\n  requires status is delivered\n"
    ));
    assert_eq!(error.code(), "name.unknown");
    assert_eq!(error.fixes().count(), 0);
    assert!(
        !error.report().to_string().contains("hint"),
        "{}",
        error.report()
    );
}

#[test]
fn a_fix_is_not_placed_where_its_target_is_ambiguous() {
    // The misspelled type appears twice in the statement the error covers.
    let error = failure(
        "system \"T\"\ntypes\n  worker is one of ann, ben\nstate\n  busy is a map from wroker to a map from wroker to a boolean\ninitially\n  busy of every worker is {ann: false, ben: false}\n",
    );
    assert_eq!(error.code(), "name.unknown");
    assert_eq!(error.fixes().count(), 0);
    assert!(
        error
            .report()
            .to_string()
            .contains("hint: did you mean `worker`?")
    );
}

#[test]
fn the_report_shows_the_fixed_line_and_where_to_read_more() {
    let error = failure(&format!(
        "{ORDERS}action pay\n  requires status is pendng   # typo\n"
    ));
    assert_eq!(
        error.report().to_string(),
        "line 17: unknown name pendng; declare it or use a quoted enum value\n  \
         hint: did you mean `pending`?\n    \
         17 |   requires status is pending   # typo\n       \
         |                      ^^^^^^^\n  \
         = see: zin explain name.unknown"
    );
    // The first line is the error's own message, unchanged.
    assert_eq!(
        error.to_string(),
        "line 17: unknown name pendng; declare it or use a quoted enum value"
    );
}

#[test]
fn fixes_point_into_the_included_file_that_holds_the_name() {
    let dir = scratch("includes");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::write(
        dir.join("main.zin"),
        "include \"parts/types.zin\"\ninclude \"parts/acount.zin\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("parts/types.zin"), "").unwrap();
    std::fs::write(
        dir.join("parts/account.zin"),
        "system \"A\"\nstate\n  open is a boolean\ninitially\n  open is flase\n",
    )
    .unwrap();
    let error = parse_file(dir.join("main.zin")).unwrap_err();
    assert_eq!(fixes(&error), fix("\"parts/account.zin\"", 2, 9, 27));
    assert!(error.fixes().all(|fix| fix.span.file.ends_with("main.zin")));

    std::fs::write(dir.join("main.zin"), "include \"parts/account.zin\"\n").unwrap();
    let error = parse_file(dir.join("main.zin"))
        .and_then(|spec| verify(&spec))
        .unwrap_err();
    assert_eq!(fixes(&error), fix("false", 5, 11, 16));
    assert!(
        error
            .fixes()
            .all(|fix| fix.span.file.ends_with("account.zin"))
    );
}

#[test]
fn check_reports_fixes_as_json() {
    let dir = scratch("json");
    let path = dir.join("orders.zin");
    std::fs::write(
        &path,
        format!("{ORDERS}action pay\n  requires status is pendng\n"),
    )
    .unwrap();
    let output = zin::cli::run(&args(&[
        "check",
        "--format",
        "json",
        path.to_str().unwrap(),
    ]));
    assert_eq!(output.code, 2);
    let json = zin::Json::parse(&output.stdout).unwrap();
    let Some(zin::Json::Array(results)) = json.get("results") else {
        panic!("{}", output.stdout);
    };
    let Some(zin::Json::Array(fixes)) = results[0].get("error").and_then(|e| e.get("fixes")) else {
        panic!("{}", output.stdout);
    };
    assert_eq!(fixes.len(), 1);
    assert_eq!(
        fixes[0].get("replacement").and_then(zin::Json::as_str),
        Some("pending")
    );
    assert_eq!(
        fixes[0].get("label").and_then(zin::Json::as_str),
        Some("did you mean `pending`?")
    );
    let span = fixes[0].get("span").unwrap();
    for (key, value) in [("line", 17), ("column", 22), ("end_column", 28)] {
        assert_eq!(span.get(key), Some(&zin::Json::Int(value)), "{key}");
    }
}

#[test]
fn check_fix_applies_single_fixes_until_the_specification_checks() {
    let dir = scratch("fix");
    let path = dir.join("orders.zin");
    let broken =
        format!("{ORDERS}action pay\r\n  requires stauts is pendng\r\n  status becomes piad\r\n");
    std::fs::write(&path, &broken).unwrap();
    let output = zin::cli::run(&args(&["check", "--fix", path.to_str().unwrap()]));
    assert_eq!(output.code, 0, "{}{}", output.stdout, output.stderr);
    let fixed = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        fixed,
        format!("{ORDERS}action pay\r\n  requires status is pending\r\n  status becomes paid\r\n")
    );
    assert_eq!(
        output.stdout.matches("fixed ").count(),
        3,
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains(":17:12: stauts -> status"),
        "{}",
        output.stdout
    );
}

#[test]
fn check_fix_leaves_alternatives_to_the_author() {
    let dir = scratch("alternatives");
    let path = dir.join("pads.zin");
    let source = "system \"T\"\nstate\n  pad is a boolean\n  pod is a boolean\ninitially\n  pad is false\n  pod is false\nalways pid or true\n";
    std::fs::write(&path, source).unwrap();
    let output = zin::cli::run(&args(&["check", "--fix", path.to_str().unwrap()]));
    assert_eq!(output.code, 2);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
    assert!(
        output.stderr.contains("hint: did you mean `pad`?"),
        "{}",
        output.stderr
    );
    assert!(
        output.stderr.contains("hint: did you mean `pod`?"),
        "{}",
        output.stderr
    );
    let output = zin::cli::run(&args(&["check", "--fix", "-"]));
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("--fix needs files"),
        "{}",
        output.stderr
    );
}

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("zin-fixes-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
