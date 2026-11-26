//! Every diagnostic code has an explanation, and each explanation's example
//! fails with exactly the output it quotes while its fix checks clean.
//! `ZIN_BLESS=1 cargo test --test explain` rewrites the quoted outputs.

use zin::{Error, ErrorKind, explanation, explanations, parse, verify};

/// The `zin check` stderr line for an error in standard input.
fn printed(error: &Error) -> String {
    if error.kind == ErrorKind::Usage {
        error.report().to_string()
    } else {
        format!("{}: {}", error.kind.label(), error.report())
    }
}

/// The text of the first fenced block of `language` in `section`.
fn block<'a>(section: &'a str, language: &str) -> Option<&'a str> {
    let start = section.find(&format!("```{language}\n"))? + language.len() + 4;
    let end = section.get(start..)?.find("```")?;
    section.get(start..start + end)
}

fn run(command: &str) -> zin::cli::Output {
    let args: Vec<String> = command
        .trim()
        .strip_prefix("$ zin ")
        .expect("a console example starts with $ zin")
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    zin::cli::run(&args)
}

/// What the example in `text` prints, and whether its fix checks clean.
fn outcome(text: &str) -> (String, Result<(), String>) {
    let (example, fix) = text.split_once("## Fix").expect("a Fix section");
    if let Some(command) = block(example, "console") {
        let output = run(command);
        let fixed = block(fix, "console").expect("a fixed command");
        // The fixed command names a file that need not exist; it must get
        // past the option that failed.
        let after = run(fixed);
        let unchanged = after.stderr == output.stderr;
        return (
            output.stderr.trim_end().to_owned(),
            if unchanged { Err(after.stderr) } else { Ok(()) },
        );
    }
    let broken = block(example, "zin").expect("a failing specification");
    let error = parse(broken)
        .and_then(|spec| verify(&spec))
        .expect_err(broken);
    let fixed = block(fix, "zin").expect("a fixed specification");
    let clean = parse(fixed)
        .and_then(|spec| verify(&spec))
        .map(drop)
        .map_err(|error| error.report().to_string());
    (printed(&error), clean)
}

#[test]
fn every_code_is_explained_by_an_example_that_produces_it() {
    let bless = std::env::var_os("ZIN_BLESS").is_some();
    for explanation in explanations() {
        let text = explanation.text;
        let (heading, rest) = text.split_once("\n\n").unwrap();
        assert_eq!(heading, format!("# `{}`", explanation.code));
        assert!(
            rest.starts_with(&format!("{}\n\n", explanation.summary)),
            "{}: the summary must follow the heading",
            explanation.code
        );
        let (output, clean) = outcome(text);
        clean.unwrap_or_else(|error| panic!("{}: the fix fails:\n{error}", explanation.code));
        let quoted = block(text, "text").expect("a quoted output");
        if bless {
            let path = format!("docs/errors/{}.md", explanation.code);
            let updated = text.replacen(
                &format!("```text\n{quoted}```"),
                &format!("```text\n{output}\n```"),
                1,
            );
            std::fs::write(path, updated).unwrap();
            continue;
        }
        assert_eq!(quoted.trim_end(), output, "{}", explanation.code);
        if explanation.code != "usage" {
            let error = parse(block(text, "zin").unwrap())
                .and_then(|spec| verify(&spec))
                .unwrap_err();
            assert_eq!(error.code(), explanation.code, "{output}");
        }
    }
}

#[test]
fn the_index_lists_every_code_with_its_summary_and_release() {
    let index = include_str!("../docs/errors/README.md");
    for explanation in explanations() {
        let row = format!(
            "| [`{0}`]({0}.md) | {1} | {2} |",
            explanation.code, explanation.summary, explanation.since
        );
        assert!(index.contains(&row), "missing row: {row}");
    }
    let rows = index
        .lines()
        .filter(|line| line.starts_with("| [`"))
        .count();
    assert_eq!(rows, explanations().len());
}

#[test]
fn every_code_an_error_reports_is_registered() {
    // Codes are declared once in src/explain.rs; `coded` takes that enum,
    // so a literal code anywhere else is a mistake.
    for file in std::fs::read_dir("src").unwrap() {
        let path = file.unwrap().path();
        if path.extension().is_none_or(|e| e != "rs") || path.ends_with("explain.rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        assert!(!source.contains("coded(\""), "{}", path.display());
    }
    for code in [
        "spec",
        "usage",
        "model.fault",
        "violation",
        "limit",
        "name.unknown",
    ] {
        assert!(explanation(code).is_some(), "{code}");
    }
    assert!(explanation("name.unknwn").is_none());
}

#[test]
fn explain_prints_an_explanation_or_lists_the_codes() {
    let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    let output = zin::cli::run(&args(&["explain", "type.mismatch"]));
    assert_eq!(output.code, 0);
    assert!(output.stdout.starts_with(
        "type.mismatch: A value of one type where another is needed. (since 0.3.0)\n"
    ));
    assert!(output.stdout.contains("## Example"));
    assert!(!output.stdout.contains("# `type.mismatch`"));

    let output = zin::cli::run(&args(&["explain"]));
    assert_eq!(output.code, 0);
    assert_eq!(output.stdout.lines().count(), explanations().len());
    assert!(output.stdout.contains("name.unknown"));

    let output = zin::cli::run(&args(&["explain", "type.mismach"]));
    assert_eq!(output.code, 2);
    assert_eq!(
        output.stderr,
        "unknown code 'type.mismach'; zin explain lists them\n  hint: did you mean `type.mismatch`?\n"
    );
}
