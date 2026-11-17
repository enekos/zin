use zin::{
    CallArguments, Result, ScenarioStep, Spec, parse, render_markdown, sequence_diagram,
    state_diagram, verify,
};

fn parse_spec(source: &str) -> Result<Spec> {
    let indentation = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.chars()
                .take_while(|character| *character == ' ')
                .count()
        })
        .min()
        .unwrap_or(0);
    let normalized = source
        .lines()
        .map(|line| line.get(indentation..).unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    parse(&normalized)
}

#[test]
fn bundled_examples_verify() {
    let account = parse(include_str!("../examples/account.zin")).expect("account parses");
    let account = verify(&account).expect("account verifies");
    assert_eq!(account.states.len(), 22);
    assert_eq!(account.transitions.len(), 182);
    assert_eq!(account.scenarios.len(), 2);

    let delivery = parse(include_str!("../examples/delivery.zin")).expect("delivery parses");
    let delivery = verify(&delivery).expect("delivery verifies");
    assert_eq!(delivery.states.len(), 6);
    assert_eq!(delivery.terminal_states(), 3);
}

#[test]
fn a_broken_promise_produces_the_shortest_counterexample() {
    let spec = parse_spec(
        r#"
        system "Counter"
        state
          count is an integer from 0 to 2
        initially
          count is 0
        always
          count is less than 2
        action increment
          requires count is less than 2
          count becomes count + 1
    "#,
    )
    .expect("spec parses");
    let error = verify(&spec).expect_err("the second increment breaks the promise");
    let message = error.to_string();
    assert!(message.contains("promise failed"), "{message}");
    assert!(message.contains("count = 2"), "{message}");
    assert!(
        message.contains("1. increment\n  2. increment"),
        "{message}"
    );
}

#[test]
fn assignments_in_an_action_are_simultaneous() {
    let spec = parse_spec(
        r#"
        system "Swap"
        state
          left is an integer from 0 to 1
          right is an integer from 0 to 1
        initially
          left is 0
          right is 1
        always
          left is at least 0
        action swap
          left becomes right
          right becomes left
        scenario "One swap"
          swap
          expect left is 1 and right is 0
    "#,
    )
    .expect("spec parses");
    verify(&spec).expect("both right-hand sides read the old state");
}

#[test]
fn named_arguments_and_multiple_parameter_domains_work() {
    let spec = parse_spec(
        r#"
        system "Named calls"
        state
          location is one of home, away
        initially
          location is home
        always
          location is home or location is away
        action move(delta is an integer from 0 to 1, target is one of home, away)
          location becomes target
        scenario "Named arguments"
          move with target = away and delta = 1
          expect location is away
    "#,
    )
    .expect("parameter boundary after enum comma is recognized");
    let result = verify(&spec).expect("named call verifies");
    assert_eq!(result.scenarios[0].calls[0].display(), "move(1, away)");
}

#[test]
fn disabled_actions_and_false_expectations_explain_the_state() {
    let source = r#"
        system "Door"
        state
          open is a boolean
        initially
          open is false
        always
          open is true or open is false
        action open_door
          requires not open
          open becomes true
        scenario "Already open"
          open_door
          open_door
    "#;
    let disabled = parse_spec(source).expect("spec parses");
    let message = verify(&disabled)
        .expect_err("second open is disabled")
        .to_string();
    assert!(message.contains("is not enabled"), "{message}");
    assert!(message.contains("open = true"), "{message}");

    let wrong_expectation = parse_spec(&source.replace(
        "          open_door\n          open_door",
        "          open_door\n          expect open is false",
    ))
    .expect("expectation spec parses");
    let message = verify(&wrong_expectation)
        .expect_err("expectation should fail")
        .to_string();
    assert!(message.contains("expectation failed"), "{message}");
    assert!(message.contains("open = true"), "{message}");
}

#[test]
fn leaving_a_declared_domain_is_a_model_error() {
    let spec = parse_spec(
        r#"
        system "Tiny counter"
        state
          count is an integer from 0 to 1
        initially
          count is 0
        action increment
          count becomes count + 1
    "#,
    )
    .expect("spec parses");
    let message = verify(&spec)
        .expect_err("two is outside the field domain")
        .to_string();
    assert!(message.contains("would assign 2"), "{message}");
}

#[test]
fn unknown_names_and_enum_typos_are_rejected() {
    let source = r#"
        system "Typo"
        state
          status is one of open, closed
        initially
          status is open
        always
          status is cloesd
    "#;
    let spec = parse_spec(source).expect("syntax is valid");
    let message = verify(&spec).expect_err("unknown enum value").to_string();
    assert!(message.contains("unknown name cloesd"), "{message}");
}

#[test]
fn logic_short_circuits_and_comments_do_not_eat_hashes_in_strings() {
    let spec = parse_spec(
        r#"
        system "A # in a name" # a real comment
        state
          count is an integer from 0 to 0
        initially
          count is 0
        always count is 0 or 1 divided by count is 1
    "#,
    )
    .expect("spec parses");
    assert_eq!(spec.name, "A # in a name");
    verify(&spec).expect("the right side is never evaluated at zero");
}

#[test]
fn state_limits_fail_loudly_instead_of_truncating_the_proof() {
    let spec = parse_spec(
        r#"
        system "Bounded exploration"
        explore at most 2 states
        state
          count is an integer from 0 to 3
        initially
          count is 0
        action increment
          requires count is less than 3
          count becomes count + 1
    "#,
    )
    .expect("spec parses");
    let message = verify(&spec)
        .expect_err("third state exceeds the limit")
        .to_string();
    assert!(message.contains("state limit"), "{message}");
}

#[test]
fn reports_embed_portable_mermaid_and_review_details() {
    let spec = parse(include_str!("../examples/account.zin")).expect("account parses");
    let verification = verify(&spec).expect("account verifies");
    let report = render_markdown(&spec, &verification);
    assert!(report.contains("```mermaid\nstateDiagram-v2"));
    assert!(report.contains("```mermaid\nsequenceDiagram"));
    assert!(report.contains("All 22 reachable states satisfy 3 promises"));
    assert!(report.contains("Deposit, withdraw, and close"));

    let diagram = state_diagram(&verification);
    assert!(diagram.contains("[*] --> s0"));
    let journey = sequence_diagram(&spec.name, &verification.scenarios[0]);
    assert!(journey.contains("reviewer->>system: deposit(5)"));
}

#[test]
fn positional_and_named_calls_remain_distinct_in_the_syntax_tree() {
    let spec = parse(include_str!("../examples/account.zin")).expect("account parses");
    let Some(ScenarioStep::Call(call)) = spec.scenarios[0].steps.first() else {
        panic!("first journey step is a call")
    };
    assert!(
        matches!(&call.arguments, CallArguments::Named(arguments) if arguments.contains_key("amount"))
    );
}

#[test]
fn unreachable_actions_are_still_fully_type_checked() {
    let cases = [
        ("count becomes false", "cannot assign a boolean"),
        ("requires count + true is 1", "expected an integer"),
    ];
    for (statement, expected) in cases {
        let source = format!(
            "system \"Static typing\"\nstate\n  count is an integer from 0 to 1\ninitially\n  count is 0\naction impossible\n  requires false\n  {statement}\n"
        );
        let spec = parse(&source).expect("spec parses");
        let message = verify(&spec)
            .expect_err("dead code must still be well typed")
            .to_string();
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn enum_domains_are_checked_even_behind_false_and_inside_quotes() {
    let source = r#"
        system "Enum safety"
        state
          status is one of open, closed
        initially
          status is open
        action impossible
          requires false
          status becomes "cloesd"
    "#;
    let spec = parse_spec(source).expect("spec parses");
    let message = verify(&spec)
        .expect_err("quoted enum typo must be rejected")
        .to_string();
    assert!(message.contains("cannot assign enum {cloesd}"), "{message}");

    let comparison = source.replace("status becomes \"cloesd\"", "requires status is \"cloesd\"");
    let spec = parse_spec(&comparison).expect("spec parses");
    let message = verify(&spec)
        .expect_err("disjoint enum comparison must be rejected")
        .to_string();
    assert!(message.contains("cannot compare"), "{message}");
}

#[test]
fn not_negates_the_complete_comparison() {
    let spec = parse_spec(
        r#"
        system "Natural negation"
        state
          status is one of open, closed
        initially
          status is closed
        always
          not status is open implies status is closed
        action close
          requires not status is open
          status becomes closed
        scenario "Already closed"
          close
          expect not status is open
    "#,
    )
    .expect("spec parses");
    verify(&spec).expect("not applies to status is open, not to status itself");
}

#[test]
fn quoted_enum_values_round_trip_in_calls_and_traces() {
    let spec = parse_spec(
        r#"
        system "Quoted values"
        state
          status is one of "needs review", "true", done
        initially
          status is "needs review"
        always
          status is "needs review" or status is "true" or status is done
        action set(target is one of "needs review", "true", done)
          status becomes target
        scenario "Quoted arguments"
          set with target = "needs review"
          set("true")
          expect status is "true"
    "#,
    )
    .expect("spec parses");
    let result = verify(&spec).expect("quoted enum values verify");
    assert_eq!(
        result.scenarios[0].calls[0].display(),
        "set(\"needs review\")"
    );
    assert_eq!(result.scenarios[0].calls[1].display(), "set(\"true\")");
}

#[test]
fn short_circuiting_does_not_hide_static_type_errors() {
    let spec = parse_spec(
        r#"
        system "Static short circuit"
        state
          count is an integer from 0 to 0
        initially
          count is 0
        always count is 0 or true + 1 is 2
    "#,
    )
    .expect("spec parses");
    let message = verify(&spec)
        .expect_err("right side is ill typed")
        .to_string();
    assert!(message.contains("expected an integer"), "{message}");
}

#[test]
fn expression_keywords_cannot_be_unreferenceable_field_names() {
    for keyword in ["true", "not", "and", "plus"] {
        let source = format!(
            "system \"Reserved names\"\nstate\n  {keyword} is a boolean\ninitially\n  {keyword} is false\n"
        );
        let message = parse(&source)
            .expect_err("reserved field name must be rejected")
            .to_string();
        assert!(message.contains("reserved for expressions"), "{message}");
    }
}

#[test]
fn signed_integer_literals_cover_the_complete_domain() {
    let spec = parse_spec(
        r#"
        system "Integer boundaries"
        state
          floor is an integer from -9223372036854775808 to -9223372036854775808
          ceiling is an integer from 9223372036854775807 to 9223372036854775807
        initially
          floor is -9223372036854775808
          ceiling is 9223372036854775807
        always
          floor is -9223372036854775808 and ceiling is 9223372036854775807
    "#,
    )
    .expect("both signed 64-bit boundaries parse");
    let result = verify(&spec).expect("boundary values verify");
    assert_eq!(result.states.len(), 1);

    let overflow = parse_spec(
        r#"
        system "Integer overflow"
        state
          count is an integer from 0 to 1
        initially
          count is 9223372036854775808
    "#,
    );
    assert!(
        overflow.is_err(),
        "a positive value above i64::MAX must be rejected"
    );
}
