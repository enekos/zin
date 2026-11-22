use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use zin::{ErrorKind, parse, parse_file, verify};

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        static ID: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "zin-includes-{}-{}",
            std::process::id(),
            ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const LIGHT: &str = "system \"Light\"\nstate\n  on is a boolean\ninitially\n  on is false\naction toggle\n  on becomes not on\n";

fn file(path: &Path) -> String {
    path.display().to_string()
}

#[test]
fn included_files_compose_like_one_file() {
    let p = Project::new();
    let entry = p.write(
        "entry.zin",
        "include \"parts/group.zin\"\ninclude \"parts/lamp.zin\"\nsystem \"Room\"\nalways on or not on\n",
    );
    p.write("parts/group.zin", "include \"lamp.zin\"\n");
    p.write("parts/lamp.zin", LIGHT);
    let spec = parse_file(&entry).unwrap();
    assert_eq!(
        spec.sources.keys().map(String::as_str).collect::<Vec<_>>(),
        ["parts/group.zin", "parts/lamp.zin"]
    );
    let flat = parse(&format!("{LIGHT}system \"Room\"\nalways on or not on\n")).unwrap();
    assert_eq!(
        verify(&flat).unwrap().summary(),
        verify(&spec).unwrap().summary()
    );
}

#[test]
fn a_file_without_includes_keeps_its_text_fingerprint() {
    let p = Project::new();
    let path = p.write("light.zin", LIGHT);
    let spec = parse_file(&path).unwrap();
    assert_eq!(spec.fingerprint, parse(LIGHT).unwrap().fingerprint);
    assert!(spec.sources.is_empty());
}

#[test]
fn editing_an_included_file_changes_the_fingerprint() {
    let p = Project::new();
    let entry = p.write("entry.zin", "include \"lamp.zin\"\n");
    let lamp = p.write("lamp.zin", LIGHT);
    let before = parse_file(&entry).unwrap();
    fs::write(&lamp, LIGHT.replace("on is false", "on is true")).unwrap();
    let after = parse_file(&entry).unwrap();
    assert_ne!(before.fingerprint, after.fingerprint);
    assert_ne!(before.sources, after.sources);
}

#[test]
fn diagnostics_name_the_included_file_and_its_line() {
    let p = Project::new();
    let entry = p.write(
        "entry.zin",
        "include \"lamp.zin\"\nsystem \"Room\"\nalways on\n",
    );
    let lamp = p.write("lamp.zin", &LIGHT.replace("not on", "not lit"));
    let error = verify(&parse_file(&entry).unwrap()).unwrap_err();
    assert_eq!(error.code(), "name.unknown");
    let span = error.source_span().unwrap();
    assert_eq!(
        (span.file.clone(), span.line, span.column, span.end_column),
        (file(&lamp), 7, 18, 21)
    );
    assert_eq!(error.line, Some(7));
    assert!(
        error
            .to_string()
            .starts_with(&format!("{}:7:18: ", file(&lamp)))
    );
}

#[test]
fn conflicting_declarations_in_two_files_report_both() {
    let p = Project::new();
    let entry = p.write(
        "entry.zin",
        "include \"lamp.zin\"\nsystem \"Other\"\nstate\n  on is a boolean\ninitially\n  on is true\n",
    );
    let lamp = p.write("lamp.zin", LIGHT);
    let error = parse_file(&entry).unwrap_err();
    assert_eq!(error.code(), "component.field_conflict");
    let span = error.source_span().unwrap();
    assert_eq!((span.file.clone(), span.line), (file(&entry), 4));
    let related = error.related_span().unwrap();
    assert_eq!(
        (related.file.clone(), related.line, related.column),
        (file(&lamp), 3, 3)
    );
}

#[test]
fn a_cycle_is_reported_at_the_include_that_closes_it() {
    let p = Project::new();
    let entry = p.write("entry.zin", "include \"a.zin\"\n");
    let a = p.write("a.zin", "include \"entry.zin\"\n");
    let error = parse_file(&entry).unwrap_err();
    assert!(error.message.contains("cyclic include"), "{error}");
    let span = error.source_span().unwrap();
    assert_eq!(
        (span.file.clone(), span.line, span.column),
        (file(&a), 1, 9)
    );
}

#[test]
fn includes_stay_within_the_entry_directory() {
    let p = Project::new();
    let entry = p.write("app/entry.zin", "include \"../outside.zin\"\n");
    p.write("outside.zin", LIGHT);
    let error = parse_file(&entry).unwrap_err();
    assert!(error.message.contains("stay within"), "{error}");
    let entry = p.write("app/entry.zin", "include \"/etc/hosts\"\n");
    assert!(
        parse_file(&entry)
            .unwrap_err()
            .message
            .contains("stay within")
    );
}

#[test]
fn includes_come_before_the_first_system() {
    let p = Project::new();
    p.write("lamp.zin", LIGHT);
    let entry = p.write("entry.zin", &format!("{LIGHT}include \"lamp.zin\"\n"));
    let error = parse_file(&entry).unwrap_err();
    assert!(error.message.contains("before the first system"), "{error}");
    assert_eq!(error.line, Some(8));
}

#[test]
fn a_missing_include_is_a_usage_error() {
    let p = Project::new();
    let entry = p.write("entry.zin", "include \"missing.zin\"\n");
    let error = parse_file(&entry).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Usage);
    assert_eq!(error.source_span().unwrap().file, file(&entry));
}

#[test]
fn text_parsing_rejects_include() {
    let error = parse("include \"lamp.zin\"\n").unwrap_err();
    assert!(error.message.contains("parse_file"), "{error}");
}

#[test]
fn a_shared_file_is_read_once() {
    let p = Project::new();
    let entry = p.write(
        "entry.zin",
        "include \"a.zin\"\ninclude \"b.zin\"\nsystem \"Room\"\nalways on or not on\n",
    );
    p.write("a.zin", "include \"lamp.zin\"\n");
    p.write("b.zin", "include \"./lamp.zin\"\n");
    p.write("lamp.zin", LIGHT);
    let spec = parse_file(&entry).unwrap();
    assert_eq!(spec.components, ["Light", "Room"]);
}
