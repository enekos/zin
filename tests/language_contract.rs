use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use zin::{
    ErrorKind, RefinementOptions, ScenarioEvent, parse, parse_file, refine, verify,
    verify_component,
};

struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "zin-source-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn write(&self, name: &str, content: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

const LIGHT: &str = "system \"Light\"\nstate\n  on is a boolean\ninitially\n  on is false\naction toggle\n  on becomes not on\n";

#[test]
fn documented_language_cases_have_both_verdicts() {
    let docs = include_str!("../docs/language-cases.md");
    let mut count = 0;
    for block in docs.split("```zin\n").skip(1) {
        let text = block.split_once("```").unwrap().0;
        verify(&parse(text).unwrap()).unwrap();
        let mut changed = false;
        let broken = text
            .lines()
            .map(|line| {
                if let Some((original, replacement)) = line.split_once(" # fails: ") {
                    changed = true;
                    format!(
                        "{}{replacement}",
                        &original[..original.len() - original.trim_start().len()]
                    )
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(changed);
        let error = parse(&broken).and_then(|s| verify(&s)).unwrap_err();
        assert!(matches!(
            error.kind,
            ErrorKind::Spec | ErrorKind::Violation | ErrorKind::Model
        ));
        count += 1;
    }
    assert_eq!(count, 4);
}

#[test]
fn files_compose_transitively_with_precise_source_diagnostics() {
    let p = Project::new();
    let root = p.write("entry.zin", "include \"parts/group.zin\"\ninclude \"parts/lamp.zin\"\nsystem \"Room\"\nalways on or not on\n");
    p.write("parts/group.zin", "include \"lamp.zin\"\n");
    let child = p.write("parts/lamp.zin", LIGHT);
    let spec = parse_file(&root).unwrap();
    assert_eq!(
        spec.sources.keys().cloned().collect::<Vec<_>>(),
        ["parts/group.zin", "parts/lamp.zin"]
    );
    let flat = parse(&format!("{LIGHT}\nsystem \"Room\"\nalways on or not on\n")).unwrap();
    assert_eq!(
        verify(&flat).unwrap().summary(),
        verify(&spec).unwrap().summary()
    );
    fs::write(&child, LIGHT.replace("not on", "not unknown")).unwrap();
    let changed = parse_file(&root).unwrap();
    assert_eq!(spec.fingerprint, changed.fingerprint);
    assert_ne!(spec.sources, changed.sources);
    let err = verify(&changed).unwrap_err();
    assert_eq!(err.code(), "name.unknown");
    let span = err.source_span().unwrap();
    assert_eq!((span.line, span.column, span.end_column), (7, 18, 25));
    assert_eq!(std::path::Path::new(&span.file), child);
    fs::write(&child, "include \"../entry.zin\"\n").unwrap();
    let cycle = parse_file(&root).unwrap_err();
    assert!(cycle.message.contains("cyclic"));
    assert_eq!(
        cycle.source_span().unwrap().file,
        child.display().to_string()
    );
    fs::write(&child, LIGHT).unwrap();
    p.write("parts/group.zin", "include \"lamp.zin\"\nsystem \"Other\"\nstate\n  on is a boolean\ninitially\n  on is true\n");
    let conflict = parse_file(&root).unwrap_err();
    assert_eq!(conflict.code(), "component.field_conflict");
    assert_eq!(conflict.source_span().unwrap().line, 4);
    assert_eq!(
        conflict.related_span().unwrap().file,
        child.display().to_string()
    );
}

#[test]
fn refinement_checks_guards_renaming_private_actions_and_limits() {
    let contract = parse(LIGHT).unwrap();
    let text = LIGHT
        .replace(
            "  on is a boolean",
            "  active is a boolean\n  tick is a boolean",
        )
        .replace("  on is false", "  active is false\n  tick is false")
        .replace("action toggle", "action flip")
        .replace("on becomes not on", "active becomes not active");
    let implementation = format!("{text}action poll\n  tick becomes not tick\n");
    let mut options = RefinementOptions::default();
    options.fields.insert("on".into(), "active".into());
    options.actions.insert("toggle".into(), "flip".into());
    assert_eq!(
        refine(&contract, &parse(&implementation).unwrap(), &options)
            .unwrap()
            .state_pairs,
        4
    );
    for (broken, message) in [
        (
            implementation.replace("active becomes not active", "active becomes true"),
            "abstract on",
        ),
        (
            implementation.replace("action flip", "action flip\n  requires not tick"),
            "concrete rejects flip",
        ),
        (
            implementation.replace("tick becomes not tick", "active becomes not active"),
            "abstract on",
        ),
    ] {
        let err = refine(&contract, &parse(&broken).unwrap(), &options).unwrap_err();
        assert_eq!(err.code(), "refinement.mismatch");
        assert!(err.message.contains(message), "{err}");
        assert!(!err.trace.unwrap().steps.is_empty());
    }
    options.max_pairs = 1;
    assert_eq!(
        refine(&contract, &parse(&implementation).unwrap(), &options)
            .unwrap_err()
            .kind,
        ErrorKind::Limit
    );
}

#[test]
fn a_component_is_driven_by_a_controllable_environment() {
    let spec = parse(include_str!("../examples/request-reply.zin")).unwrap();
    let client = verify_component(&spec, "Client").unwrap();
    assert_eq!(client.model.component.as_deref(), Some("Client"));
    assert_eq!(client.model.fields.len(), 2);
    assert!(client.model.action_index("work").is_none());
    let steps: Vec<_> = client.scenarios[0]
        .events
        .iter()
        .filter_map(|event| match event {
            ScenarioEvent::Call { call, .. } => Some(client.call_text(*call)),
            _ => None,
        })
        .collect();
    assert_eq!(steps, ["request(2)", "reply(2)"]);
    // The global scenario refuses the early reply before the server finishes.
    // The client allows it, so this refusal must never reach the client adapter.
    assert!(!client.scenarios[0].events.iter().any(|event| matches!(event, ScenarioEvent::Reject{call} if client.call_text(*call) == "reply(2)")));
}
