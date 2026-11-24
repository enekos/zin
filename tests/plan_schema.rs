//! Every plan `zin plan` writes, and every committed plan fixture, matches
//! docs/plan.schema.json: the one definition that the runners also read.
use std::path::{Path, PathBuf};
use zin::{Cover, Json, Plan, PlanOptions, parse_file, verify, verify_component};

const HEX16: &str = "^[0-9a-f]{16}$";

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn schema() -> Json {
    let text = std::fs::read_to_string(root().join("docs/plan.schema.json")).unwrap();
    Json::parse(&text).unwrap()
}

fn entries(json: &Json) -> &[(String, Json)] {
    match json {
        Json::Object(entries) => entries,
        _ => &[],
    }
}

fn number(schema: &Json, key: &str) -> Option<i64> {
    match schema.get(key) {
        Some(Json::Int(n)) => Some(*n),
        _ => None,
    }
}

fn type_matches(name: &str, value: &Json) -> bool {
    matches!(
        (name, value),
        ("object", Json::Object(_))
            | ("array", Json::Array(_))
            | ("string", Json::Str(_))
            | ("integer", Json::Int(_))
            | ("boolean", Json::Bool(_))
            | ("null", Json::Null)
    )
}

/// A deliberately small subset of JSON Schema: the keywords the plan schema
/// uses. An unknown keyword or pattern fails loudly rather than passing.
fn validate(root: &Json, schema: &Json, value: &Json, at: &str, errors: &mut Vec<String>) {
    for (keyword, rule) in entries(schema) {
        match keyword.as_str() {
            "$schema" | "$id" | "title" | "description" | "$defs" => {}
            "$ref" => {
                let name = rule
                    .as_str()
                    .and_then(|r| r.strip_prefix("#/$defs/"))
                    .expect("local $ref");
                let target = root
                    .get("$defs")
                    .and_then(|d| d.get(name))
                    .expect("defined");
                validate(root, target, value, at, errors);
            }
            "type" => {
                let names: Vec<&str> = match rule {
                    Json::Str(name) => vec![name.as_str()],
                    Json::Array(names) => names.iter().filter_map(Json::as_str).collect(),
                    _ => panic!("type at {at}"),
                };
                if !names.iter().any(|name| type_matches(name, value)) {
                    errors.push(format!("{at}: expected {names:?}"));
                }
            }
            "enum" => {
                let Json::Array(allowed) = rule else {
                    panic!("enum at {at}")
                };
                if !allowed.contains(value) {
                    errors.push(format!("{at}: {} is not allowed", value.to_compact()));
                }
            }
            "required" => {
                let Json::Array(keys) = rule else {
                    panic!("required at {at}")
                };
                if let Json::Object(_) = value {
                    for key in keys.iter().filter_map(Json::as_str) {
                        if value.get(key).is_none() {
                            errors.push(format!("{at}: missing {key}"));
                        }
                    }
                }
            }
            "properties" => {
                for (key, item) in entries(value) {
                    if let Some(property) = rule.get(key) {
                        validate(root, property, item, &format!("{at}.{key}"), errors);
                    }
                }
            }
            "additionalProperties" => {
                let known = schema.get("properties");
                for (key, item) in entries(value) {
                    if known.and_then(|k| k.get(key)).is_none() {
                        validate(root, rule, item, &format!("{at}.{key}"), errors);
                    }
                }
            }
            "items" => {
                if let Json::Array(items) = value {
                    for (index, item) in items.iter().enumerate() {
                        validate(root, rule, item, &format!("{at}[{index}]"), errors);
                    }
                }
            }
            "minItems" | "maxItems" => {
                if let Json::Array(items) = value {
                    let bound = usize::try_from(number(schema, keyword).unwrap()).unwrap();
                    let fine = if keyword == "minItems" {
                        items.len() >= bound
                    } else {
                        items.len() <= bound
                    };
                    if !fine {
                        errors.push(format!("{at}: {keyword} {bound}"));
                    }
                }
            }
            "minimum" => {
                if let Json::Int(n) = value
                    && *n < number(schema, "minimum").unwrap()
                {
                    errors.push(format!("{at}: {n} is below the minimum"));
                }
            }
            "pattern" => {
                assert_eq!(rule.as_str(), Some(HEX16), "unsupported pattern at {at}");
                if let Json::Str(text) = value
                    && !(text.len() == 16
                        && text
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
                {
                    errors.push(format!("{at}: {text} is not 16 lowercase hex digits"));
                }
            }
            other => panic!("the test validator does not support {other} (at {at})"),
        }
    }
}

fn violations(plan: &Json) -> Vec<String> {
    let schema = schema();
    let mut errors = Vec::new();
    validate(&schema, &schema, plan, "plan", &mut errors);
    errors
}

fn assert_valid(plan: &Json, origin: &str) {
    let errors = violations(plan);
    assert!(errors.is_empty(), "{origin}: {errors:#?}");
}

fn specs(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "zin"))
        .collect();
    found.sort();
    found
}

#[test]
fn every_example_plan_matches_the_schema_for_every_coverage() {
    let mut checked = 0;
    for spec in specs(&root().join("examples")) {
        let parsed = parse_file(&spec).unwrap();
        let verification = verify(&parsed).unwrap();
        for cover in [
            Cover::Transitions,
            Cover::States,
            Cover::Actions,
            Cover::Scenarios,
            Cover::Pairs { limit: None },
            Cover::Pairs { limit: Some(3) },
        ] {
            let Ok(plan) = Plan::build(&verification, &PlanOptions::cover(cover)) else {
                continue;
            };
            assert_valid(
                &plan.to_json(&verification),
                &format!("{} --cover {}", spec.display(), cover.name()),
            );
            checked += 1;
        }
        for observed in [Vec::new(), vec![verification.model.fields[0].name.clone()]] {
            let plan =
                Plan::build_observing(&verification, &PlanOptions::default(), &observed).unwrap();
            assert_valid(
                &plan.to_json(&verification),
                &format!("{} --observe {observed:?}", spec.display()),
            );
            checked += 1;
        }
        for component in &parsed.components {
            // A block without state of its own has nothing to test in isolation.
            let Ok(isolated) = verify_component(&parsed, component) else {
                continue;
            };
            let plan = Plan::build(&isolated, &PlanOptions::default()).unwrap();
            assert_valid(
                &plan.to_json(&isolated),
                &format!("{} --component {component}", spec.display()),
            );
            checked += 1;
        }
    }
    assert!(checked > 20, "only {checked} plans checked");
}

#[test]
fn committed_plan_fixtures_match_the_schema() {
    let mut paths = vec![root().join("tests/fixtures/v0.2/switch.plan.json")];
    let mut pending = vec![root().join("bindings/fixtures/contract")];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.to_string_lossy().ends_with(".plan.json") {
                paths.push(path);
            }
        }
    }
    assert!(paths.len() > 3);
    for path in paths {
        let plan = Json::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_valid(&plan, &path.display().to_string());
    }
}

#[test]
fn the_schema_rejects_malformed_plans() {
    let spec = parse_file(root().join("bindings/fixtures/contract/latch.zin")).unwrap();
    let verification = verify(&spec).unwrap();
    let text = Plan::build(&verification, &PlanOptions::default())
        .unwrap()
        .to_json(&verification)
        .to_compact();
    for (from, to, wanted) in [
        ("\"tests\":", "\"checks\":", "missing tests"),
        ("[0,1]", "[0,1,2]", "maxItems 2"),
        ("\"plan/1\"", "\"plan/3\"", "is not allowed"),
        (
            "\"kind\":\"scenario\"",
            "\"kind\":\"random\"",
            "is not allowed",
        ),
    ] {
        assert!(text.contains(from), "{from}");
        let broken = Json::parse(&text.replacen(from, to, 1)).unwrap();
        let errors = violations(&broken);
        assert!(
            errors.iter().any(|error| error.contains(wanted)),
            "{from} -> {to}: {errors:?}"
        );
    }
}
