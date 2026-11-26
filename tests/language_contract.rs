use zin::{ErrorKind, parse, verify};

/// Each block in docs/language-cases.md passes. Replacing the line marked
/// `# fails:` with the text after it must produce a diagnostic located on
/// that line, a model fault there, or a violation with a counterexample.
#[test]
fn documented_language_cases_have_both_verdicts() {
    let docs = include_str!("../docs/language-cases.md");
    let mut count = 0;
    for block in docs.split("```zin\n").skip(1) {
        let text = block.split_once("```").unwrap().0;
        verify(&parse(text).unwrap()).unwrap();
        let mut changed = None;
        let broken = text
            .lines()
            .enumerate()
            .map(|(index, line)| match line.split_once(" # fails: ") {
                Some((original, replacement)) => {
                    changed = Some(index + 1);
                    let indent = original.len() - original.trim_start().len();
                    format!("{}{replacement}", &original[..indent])
                }
                None => line.to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        let changed = changed.expect("a case without a failing variant");
        let error = parse(&broken).and_then(|s| verify(&s)).unwrap_err();
        match error.kind {
            ErrorKind::Spec | ErrorKind::Model => {
                assert_eq!(error.line, Some(changed), "{error}");
                assert_eq!(error.source_span().unwrap().line, changed, "{error}");
            }
            ErrorKind::Violation => assert!(error.trace.is_some(), "{error}"),
            _ => panic!("unexpected {error}"),
        }
        count += 1;
    }
    assert_eq!(count, 10);
}
