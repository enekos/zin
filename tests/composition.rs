use std::collections::BTreeSet;
use zin::ast::Fairness;
use zin::{ErrorKind, Verification, parse, verify};

fn check(source: &str) -> zin::Result<Verification> {
    verify(&parse(source)?)
}

fn failure(source: &str) -> zin::Error {
    check(source).expect_err("the composition should be rejected")
}

const REQUEST_REPLY: &str = include_str!("../examples/request-reply.zin");

/// The same system as `examples/request-reply.zin`, written by hand as one block.
const FLAT: &str = r#"system "Flat"
state
  waiting is a boolean
  asked is an integer from 0 to 3
  job is an integer from 0 to 3
  done is a boolean
initially
  waiting is false
  asked is 0
  job is 0
  done is false
action request(n is an integer from 1 to 3)
  requires not waiting
  requires job is 0
  waiting becomes true
  asked becomes n
  job becomes n
fair action reply(n is an integer from 1 to 3)
  requires waiting and asked is n
  requires done and job is n
  waiting becomes false
  job becomes 0
  done becomes false
fair action work
  requires job is not 0 and not done
  done becomes true
"#;

fn transitions(verification: &Verification) -> BTreeSet<String> {
    let mut all = BTreeSet::new();
    for state in 0..verification.graph.state_count() {
        let from = u32::try_from(state).unwrap();
        for edge in verification.graph.out(from) {
            all.insert(format!(
                "{} --{}--> {}",
                zin::display_state(&verification.state(from)),
                verification.call_text(edge.call),
                zin::display_state(&verification.state(edge.to))
            ));
        }
    }
    all
}

#[test]
fn a_composition_is_the_synchronized_product_of_its_components() {
    let composed = check(REQUEST_REPLY).unwrap();
    let flat = check(FLAT).unwrap();
    assert_eq!(composed.model.name, "Request and reply");
    assert_eq!(transitions(&composed), transitions(&flat));
    assert_eq!(composed.properties.len(), 4);
    assert_eq!(composed.scenarios.len(), 1);
    let reply = composed.model.action_index("reply").unwrap();
    assert_eq!(composed.model.actions[reply].fairness, Fairness::Weak);
    let request = composed.model.action_index("request").unwrap();
    assert_eq!(composed.model.actions[request].fairness, Fairness::Unfair);
}

#[test]
fn a_bug_in_one_component_breaks_a_property_of_the_whole() {
    let broken = REQUEST_REPLY.replace("  job becomes n\n", "  job becomes 1\n");
    let error = failure(&broken);
    assert_eq!(error.kind, ErrorKind::Violation);
    assert!(
        error
            .message
            .contains("'always waiting implies job is asked'"),
        "{error}"
    );
    let trace = error.trace.expect("a counterexample");
    assert_eq!(trace.steps.len(), 1);
    assert_eq!(trace.steps[0].call, "request(2)");
}

#[test]
fn components_are_named_after_their_blocks_without_an_observer() {
    let source = REQUEST_REPLY
        .split("system \"Request and reply\"")
        .next()
        .unwrap();
    assert_eq!(check(source).unwrap().model.name, "Client + Server");
}

#[test]
fn components_share_no_state() {
    let clash = REQUEST_REPLY.replace(
        "  job is an integer from 0 to 3\n",
        "  asked is an integer from 0 to 3\n",
    );
    let error = failure(&clash);
    assert!(
        error.message.contains("already a state field of Client"),
        "{error}"
    );

    let reads = REQUEST_REPLY.replace(
        "  requires waiting and asked is n",
        "  requires waiting and asked is n and job is n",
    );
    let error = failure(&reads);
    assert_eq!(error.line, Some(22));
    assert!(
        error
            .message
            .contains("action reply of Client reads job, which belongs to Server"),
        "{error}"
    );

    let writes = REQUEST_REPLY.replace(
        "  done becomes true\n",
        "  done becomes true\n  waiting becomes false\n",
    );
    let error = failure(&writes);
    assert!(error.message.contains("assigns waiting"), "{error}");

    let initial = REQUEST_REPLY.replace("  done is false\n", "  done is false\n  asked is 1\n");
    let error = failure(&initial);
    assert!(error.message.contains("initial value of asked"), "{error}");
}

#[test]
fn shared_actions_agree_on_parameters_and_types() {
    let error = failure(&REQUEST_REPLY.replacen(
        "fair action reply(n is an integer from 1 to 3)",
        "fair action reply(n is a boolean)",
        1,
    ));
    assert!(
        error
            .message
            .contains("parameter n of the shared action reply"),
        "{error}"
    );

    let typed = |client: &str, server: &str| {
        REQUEST_REPLY
            .replacen("state\n", &format!("types\n  {client}\nstate\n"), 1)
            .replacen(
                "state\n  job",
                &format!("types\n  {server}\nstate\n  job"),
                1,
            )
    };
    check(&typed(
        "size is an integer from 0 to 3",
        "size is an integer from 0 to 3",
    ))
    .unwrap();
    let error = failure(&typed(
        "size is an integer from 0 to 3",
        "size is an integer from 0 to 4",
    ));
    assert!(
        error.message.contains("declares type size differently"),
        "{error}"
    );
}

#[test]
fn components_have_distinct_names_and_actions() {
    let error = failure(&REQUEST_REPLY.replace("system \"Server\"", "system \"Client\""));
    assert!(error.message.contains("both called \"Client\""), "{error}");
    let error = failure(&REQUEST_REPLY.replace("fair action work", "action request"));
    assert!(
        error.message.contains("duplicate action request in Server"),
        "{error}"
    );
}

#[test]
fn the_strongest_fairness_of_a_shared_action_wins() {
    let strong = REQUEST_REPLY.replace(
        "action reply(n is an integer from 1 to 3)\n  requires waiting",
        "strongly fair action reply(n is an integer from 1 to 3)\n  requires waiting",
    );
    let verification = check(&strong).unwrap();
    let reply = verification.model.action_index("reply").unwrap();
    assert_eq!(verification.model.actions[reply].fairness, Fairness::Strong);
}

#[test]
fn a_component_is_driven_by_a_controllable_environment() {
    let spec = zin::parse(include_str!("../examples/request-reply.zin")).unwrap();
    let client = zin::verify_component(&spec, "Client").unwrap();
    assert_eq!(client.model.component.as_deref(), Some("Client"));
    assert_eq!(client.model.fields.len(), 2);
    assert!(client.model.action_index("work").is_none());
    let calls: Vec<_> = client.scenarios[0]
        .events
        .iter()
        .filter_map(|event| match event {
            zin::ScenarioEvent::Call { call, .. } => Some(client.call_text(*call)),
            _ => None,
        })
        .collect();
    assert_eq!(calls, ["request(2)", "reply(2)"]);
    // The whole system refuses an early reply because the server is still
    // working. The client alone allows it, so that refusal never reaches it.
    assert!(!client.scenarios[0].events.iter().any(|event| matches!(
        event,
        zin::ScenarioEvent::Reject { call } if client.call_text(*call) == "reply(2)"
    )));
    let plan = zin::Plan::build(&client, &zin::PlanOptions::default()).unwrap();
    let json = plan.to_json(&client).to_compact();
    assert!(json.contains("\"component\":\"Client\""), "{json}");
}

#[test]
fn an_unknown_or_stateless_component_is_a_usage_error() {
    let spec = zin::parse(include_str!("../examples/request-reply.zin")).unwrap();
    let error = zin::verify_component(&spec, "Nobody").unwrap_err();
    assert_eq!(error.kind, zin::ErrorKind::Usage);
    assert!(error.message.contains("Client"), "{error}");
    let error = zin::verify_component(&spec, "Request and reply").unwrap_err();
    assert!(error.message.contains("no state"), "{error}");
}
