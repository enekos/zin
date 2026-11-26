//! The `zin` command line, as a library function so it can be tested.

use crate::audit::{MutantFate, audit};
use crate::check::{Options, Verification, verify, verify_with};
use crate::induction::{Broken, Induction, check_induction};
use crate::json::Json;
use crate::mermaid::{sequence_diagram, state_diagram};
use crate::observe::adequacy;
use crate::plan::{Cover, Plan, PlanOptions, TestKind, state_json};
use crate::report::render_markdown;
use crate::scaffold::{Framework, scaffold};
use crate::trace::check_trace_reader;
use crate::{Error, ErrorKind, Result, SourceSpan, Spec, parse, parse_file};
use std::fmt::Write as _;
use std::io::Read as _;

const MAX_SPEC_BYTES: u64 = crate::source::MAX_SPEC_BYTES as u64;
const MAX_TRACE_BYTES: u64 = 1024 * 1024 * 1024;

pub const USAGE: &str = "\
zin - specifications you can prove, then run against your code

usage:
  zin check <spec>... [--format json] [--symmetry on|off] [--inductive]
                                        prove every property over every reachable state;
                                        --inductive also asks whether every call from any
                                        state satisfying the always/never properties keeps them
  zin plan <spec> [--out FILE] [--cover transitions|states|actions|scenarios|pairs] [--max-steps N] [--step-budget N] [--pair-limit N]
           [--component NAME] [--observe FIELDS]
                                        derive the conformance test plan (JSON);
                                        --component NAME tests one block of a composition;
                                        --observe a,b adds tests that confirm each state
                                        reached when observe() returns only those fields
  zin observe <spec> [--fields a,b] [--component NAME]
                                        which states a test run can tell apart when observe()
                                        returns those fields (none: accepted/rejected only)
  zin trace <spec> <events.jsonl|-> [--symmetry on|off]
                                        check a recorded execution against the specification
  zin audit <spec>                      mutate the specification; list changes nothing notices
  zin init <framework> <spec>           print a conformance test skeleton
                                        (pytest, unittest, jest, vitest, node, playwright,
                                        testing-library, go, rust, junit)
  zin report <spec>                     Markdown report with Mermaid diagrams
  zin diagram <spec> [scenario]         Mermaid state diagram, or one scenario's sequence diagram

exit codes: 0 ok, 1 the specification or implementation is wrong, 2 the input could not be used";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub code: u8,
    pub stdout: String,
    pub stderr: String,
}

struct Arguments {
    positional: Vec<String>,
    options: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Arguments {
    fn parse(args: &[String], valued: &[&str]) -> Result<Self> {
        Self::parse_with_flags(args, valued, &[])
    }

    /// As [`Arguments::parse`], where the options in `flags` take no value.
    fn parse_with_flags(args: &[String], valued: &[&str], flags: &[&str]) -> Result<Self> {
        let mut parsed = Self {
            positional: Vec::new(),
            options: Vec::new(),
            flags: Vec::new(),
        };
        let mut rest = args.iter();
        while let Some(argument) = rest.next() {
            if let Some(name) = argument.strip_prefix("--") {
                if flags.contains(&name) {
                    parsed.flags.push(name.to_string());
                    continue;
                }
                if !valued.contains(&name) {
                    return Err(Error::usage(format!("unknown option --{name}")));
                }
                let value = rest
                    .next()
                    .ok_or_else(|| Error::usage(format!("--{name} needs a value")))?;
                parsed.options.push((name.to_string(), value.clone()));
            } else {
                parsed.positional.push(argument.clone());
            }
        }
        Ok(parsed)
    }

    fn option(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(option, _)| option == name)
            .map(|(_, value)| value.as_str())
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    fn one(&self, what: &str) -> Result<&str> {
        match self.positional.as_slice() {
            [only] => Ok(only),
            _ => Err(Error::usage(format!("expected exactly one {what}"))),
        }
    }
}

fn read(path: &str, limit: u64) -> Result<String> {
    let fail = |error: std::io::Error| Error::usage(format!("cannot read {path}: {error}"));
    let mut text = String::new();
    if path == "-" {
        std::io::stdin()
            .take(limit + 1)
            .read_to_string(&mut text)
            .map_err(fail)?;
    } else {
        std::fs::File::open(path)
            .map_err(fail)?
            .take(limit + 1)
            .read_to_string(&mut text)
            .map_err(fail)?;
    }
    if u64::try_from(text.len()).unwrap_or(u64::MAX) > limit {
        return Err(Error::usage(format!("{path} is larger than {limit} bytes")));
    }
    Ok(text)
}

fn load(path: &str) -> Result<Verification> {
    verify(&load_spec(path)?)
}

/// A file with its includes, or a self-contained specification on standard input.
fn load_spec(path: &str) -> Result<Spec> {
    if path == "-" {
        parse(&read(path, MAX_SPEC_BYTES)?)
    } else {
        parse_file(path)
    }
}

fn span_json(span: &SourceSpan) -> Json {
    Json::object([
        ("file", Json::str(&span.file)),
        ("line", Json::count(span.line)),
        ("column", Json::count(span.column)),
        ("end_column", Json::count(span.end_column)),
    ])
}

fn error_json(error: &Error) -> Json {
    let trace = error.trace.as_ref().map_or(Json::Null, |trace| {
        Json::object([
            ("initial", state_json(&trace.initial)),
            (
                "steps",
                Json::Array(
                    trace
                        .steps
                        .iter()
                        .map(|step| {
                            Json::object([
                                ("call", Json::str(step.call.clone())),
                                ("state", state_json(&step.state)),
                            ])
                        })
                        .collect(),
                ),
            ),
            ("loop_from", trace.loop_from.map_or(Json::Null, Json::count)),
        ])
    });
    Json::object([
        ("kind", Json::str(error.kind.label())),
        ("code", Json::str(error.code())),
        ("line", error.line.map_or(Json::Null, Json::count)),
        ("span", error.source_span().map_or(Json::Null, span_json)),
        (
            "related",
            error.related_span().map_or(Json::Null, span_json),
        ),
        ("message", Json::str(error.message.clone())),
        ("trace", trace),
    ])
}

fn check_json(path: &str, spec: Option<&Spec>, result: &Result<Checked>) -> Json {
    match result {
        Ok((verification, induction)) => {
            let mut json = verification_json(path, spec, verification);
            if let (Json::Object(entries), Some(induction)) = (&mut json, induction) {
                entries.push(("inductive".to_string(), induction_json(spec, induction)));
            }
            json
        }
        Err(error) => Json::object([
            ("file", Json::str(path)),
            ("ok", Json::Bool(false)),
            ("error", error_json(error)),
        ]),
    }
}

/// A verified specification and, when asked for, whether its properties are inductive.
type Checked = (Verification, Option<Induction>);

fn verification_json(path: &str, spec: Option<&Spec>, verification: &Verification) -> Json {
    Json::object([
        ("file", Json::str(path)),
        ("ok", Json::Bool(true)),
        ("system", Json::str(verification.model.name.clone())),
        ("states", Json::count(verification.graph.state_count())),
        ("calls", Json::count(verification.model.call_count())),
        (
            "attempted_calls",
            Json::count(
                verification
                    .graph
                    .state_count()
                    .saturating_mul(verification.model.call_count()),
            ),
        ),
        ("relabelings", Json::count(verification.graph.relabelings())),
        (
            "transitions",
            Json::count(verification.graph.transition_count()),
        ),
        ("up_to_symmetry", Json::Bool(verification.reduced)),
        ("terminal_states", Json::count(verification.terminal_states)),
        (
            "properties",
            Json::Array(
                verification
                    .properties
                    .iter()
                    .map(|property| {
                        let span = spec.and_then(|spec| spec.statement_span(property.line));
                        Json::object([
                            ("property", Json::str(property.text.clone())),
                            (
                                "line",
                                Json::count(span.as_ref().map_or(property.line, |s| s.line)),
                            ),
                            ("span", span.as_ref().map_or(Json::Null, span_json)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "scenarios",
            Json::Array(
                verification
                    .scenarios
                    .iter()
                    .map(|scenario| Json::str(scenario.name.clone()))
                    .collect(),
            ),
        ),
        (
            "notes",
            Json::Array(verification.notes.iter().map(Json::str).collect()),
        ),
    ])
}

/// The file and line of a statement, or `line N` for standard input.
fn statement_line(spec: Option<&Spec>, line: usize) -> (String, usize) {
    spec.and_then(|spec| spec.statement_span(line))
        .map_or((String::new(), line), |span| (span.file, span.line))
}

fn induction_json(spec: Option<&Spec>, induction: &Induction) -> Json {
    let counterexample = induction
        .counterexample
        .as_ref()
        .map_or(Json::Null, |found| {
            let (property, message, line) = match &found.broken {
                Broken::Property { text, line } => (Json::str(text.clone()), Json::Null, *line),
                Broken::Fault { message, line } => (Json::Null, Json::str(message.clone()), *line),
            };
            let (file, line) = statement_line(spec, line);
            Json::object([
                ("state", state_json(&found.state)),
                ("call", Json::str(found.call.clone())),
                ("next", found.next.as_ref().map_or(Json::Null, state_json)),
                ("property", property),
                ("fault", message),
                ("file", Json::str(file)),
                ("line", Json::count(line)),
            ])
        });
    Json::object([
        ("holds", Json::Bool(induction.holds())),
        ("properties", Json::count(induction.properties)),
        (
            "well_typed_states",
            Json::count(induction.well_typed_states),
        ),
        (
            "satisfying_states",
            Json::count(induction.satisfying_states),
        ),
        ("counterexample", counterexample),
    ])
}

fn induction_text(spec: Option<&Spec>, induction: &Induction) -> String {
    if induction.properties == 0 {
        return "  note: there are no always or never properties to check for induction\n"
            .to_string();
    }
    let (properties, hold) = if induction.properties == 1 {
        ("the always/never property".to_string(), "holds")
    } else {
        (
            format!("the {} always/never properties", induction.properties),
            "hold",
        )
    };
    let Some(found) = &induction.counterexample else {
        return format!(
            "  inductive: {properties} still {hold} after every call from any of the {} of {} well-typed states where {} {hold}\n",
            induction.satisfying_states,
            induction.well_typed_states,
            if induction.properties == 1 {
                "it"
            } else {
                "they"
            },
        );
    };
    let place = |line: usize| match statement_line(spec, line) {
        (file, line) if !file.is_empty() => format!("{file}:{line}"),
        (_, line) => format!("line {line}"),
    };
    let mut text = match &found.broken {
        Broken::Property { text, line } => format!(
            "  not inductive: {} breaks '{text}' ({}) from a state where {properties} {hold}\n",
            found.call,
            place(*line)
        ),
        Broken::Fault { message, line } => format!(
            "  not inductive: {} faults ({}) from a state where {properties} {hold}: {message}\n",
            found.call,
            place(*line)
        ),
    };
    let _ = writeln!(text, "    state: {}", crate::display_state(&found.state));
    let _ = writeln!(
        text,
        "    {}  ->  {}",
        found.call,
        found.next.as_ref().map_or_else(
            || "fault".to_string(),
            |next| crate::display_changes(&found.state, next)
        )
    );
    text.push_str(match found.broken {
        Broken::Property { .. } => {
            "  no reachable state looks like this one, but the properties do not rule it out; add an 'always' that does, so that each call preserves them on its own\n"
        }
        Broken::Fault { .. } => {
            "  no reachable state looks like this one, but the properties do not rule it out; add an 'always' that does, or a 'requires' that refuses the call there\n"
        }
    });
    text
}

/// How `check` and `trace` verify: with symmetry reduction unless `--symmetry off`.
fn verify_options(arguments: &Arguments) -> Result<Options> {
    let symmetry = match arguments.option("symmetry") {
        None | Some("on") => true,
        Some("off") => false,
        Some(other) => {
            return Err(Error::usage(format!(
                "--symmetry is on or off, not '{other}'"
            )));
        }
    };
    Ok(Options { symmetry })
}

fn check(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse_with_flags(args, &["format", "symmetry"], &["inductive"])?;
    let inductive = arguments.flag("inductive");
    let json = match arguments.option("format") {
        None | Some("text") => false,
        Some("json") => true,
        Some(other) => return Err(Error::usage(format!("unknown format '{other}'"))),
    };
    let options = verify_options(&arguments)?;
    if arguments.positional.is_empty() {
        return Err(Error::usage("check needs at least one specification file"));
    }
    let mut output = Output {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    let mut results = Vec::new();
    for path in &arguments.positional {
        let spec = load_spec(path);
        let result = spec
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|spec| verify_with(spec, options))
            .and_then(|verification| {
                let induction = inductive
                    .then(|| check_induction(&verification))
                    .transpose()?;
                Ok((verification, induction))
            });
        if let Err(error) = &result {
            output.code = output.code.max(error.kind.exit_code());
        }
        if json {
            results.push(check_json(path, spec.as_ref().ok(), &result));
            continue;
        }
        match result {
            Ok((verification, induction)) => {
                let _ = writeln!(output.stdout, "ok {}", verification.summary());
                for note in &verification.notes {
                    let _ = writeln!(output.stdout, "  note: {note}");
                }
                if let Some(induction) = &induction {
                    output
                        .stdout
                        .push_str(&induction_text(spec.as_ref().ok(), induction));
                }
            }
            Err(error)
                if error
                    .source_span()
                    .is_some_and(|span| !span.file.is_empty()) =>
            {
                let _ = writeln!(output.stderr, "{}: {error}", error.kind.label());
            }
            Err(error) => {
                let _ = writeln!(output.stderr, "{path}: {}: {error}", error.kind.label());
            }
        }
    }
    if json {
        let _ = writeln!(
            output.stdout,
            "{}",
            Json::object([
                ("zin", Json::str("check/1")),
                ("results", Json::Array(results)),
            ])
            .to_compact()
        );
    }
    Ok(output)
}

fn plan(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(
        args,
        &[
            "out",
            "cover",
            "max-steps",
            "step-budget",
            "pair-limit",
            "component",
            "observe",
        ],
    )?;
    let mut options = PlanOptions::default();
    if let Some(cover) = arguments.option("cover") {
        options.cover = Cover::parse(cover)?;
    }
    let positive = |name: &str| -> Result<Option<usize>> {
        arguments
            .option(name)
            .map(|value| {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or_else(|| Error::usage(format!("--{name} needs a positive integer")))
            })
            .transpose()
    };
    options.max_steps = positive("max-steps")?.unwrap_or(options.max_steps);
    options.step_budget = positive("step-budget")?.unwrap_or(options.step_budget);
    if let Some(limit) = positive("pair-limit")? {
        let Cover::Pairs { .. } = options.cover else {
            return Err(Error::usage("--pair-limit needs --cover pairs"));
        };
        options.cover = Cover::Pairs { limit: Some(limit) };
    }
    let spec = load_spec(arguments.one("specification file")?)?;
    let verification = match arguments.option("component") {
        Some(name) => crate::verify_component(&spec, name)?,
        None => verify(&spec)?,
    };
    let plan = match arguments.option("observe") {
        Some(fields) => Plan::build_observing(&verification, &options, &field_list(fields))?,
        None => Plan::build(&verification, &options)?,
    };
    let blind = plan
        .identification
        .as_ref()
        .map_or(0, |identification| identification.blind_states);
    let stderr = if blind > 0 {
        format!(
            "warning: {} cannot be told from another under this observation; run zin observe\n",
            crate::counted(blind, "state", "states")
        )
    } else {
        String::new()
    };
    let mut text = plan.to_json(&verification).to_compact();
    text.push('\n');
    match arguments.option("out") {
        Some(path) => {
            std::fs::write(path, text)
                .map_err(|error| Error::usage(format!("cannot write {path}: {error}")))?;
            Ok(Output {
                code: 0,
                stdout: format!(
                    "ok {}\nplan: {} -> {path}\n",
                    verification.summary(),
                    plan.summary()
                ),
                stderr,
            })
        }
        None => Ok(Output {
            code: 0,
            stdout: text,
            stderr,
        }),
    }
}

/// The names in a comma-separated option value; an empty value names none.
fn field_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

fn observe(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &["fields", "component"])?;
    let fields = field_list(arguments.option("fields").unwrap_or(""));
    let spec = load_spec(arguments.one("specification file")?)?;
    let verification = match arguments.option("component") {
        Some(name) => crate::verify_component(&spec, name)?,
        None => verify(&spec)?,
    };
    let adequacy = adequacy(&verification, &fields)?;
    let mut stdout = String::new();
    let _ = writeln!(stdout, "ok {}", verification.summary());
    let observing = if adequacy.fields.is_empty() {
        "no fields (accepted and rejected calls only)".to_string()
    } else {
        adequacy.fields.join(", ")
    };
    let _ = writeln!(
        stdout,
        "observing {observing}: {} show {}",
        crate::counted(adequacy.states, "state", "states"),
        crate::counted(
            adequacy.observations,
            "distinct observation",
            "distinct observations"
        )
    );
    if let Some(hardest) = &adequacy.hardest {
        let _ = writeln!(
            stdout,
            "states that look alike are told apart by at most {}, for example",
            crate::counted(adequacy.depth, "further call", "further calls")
        );
        let _ = writeln!(stdout, "  {}", crate::display_state(&hardest.first));
        let _ = writeln!(stdout, "  {}", crate::display_state(&hardest.second));
        let _ = writeln!(stdout, "  by: {}", hardest.calls.join(", "));
    }
    for group in &adequacy.blind {
        let _ = writeln!(
            stdout,
            "BLIND {} that no sequence of calls tells apart, for example",
            crate::counted(group.states, "state", "states")
        );
        for state in &group.examples {
            let _ = writeln!(stdout, "  {}", crate::display_state(state));
        }
        let it = if group.differ.len() == 1 {
            "it"
        } else {
            "them"
        };
        let _ = writeln!(
            stdout,
            "  they differ in {}: observe {it}, or the specification does not need {it}",
            group.differ.join(", ")
        );
    }
    if adequacy.blind_groups > adequacy.blind.len() {
        let _ = writeln!(
            stdout,
            "... and {} more blind groups",
            adequacy.blind_groups - adequacy.blind.len()
        );
    }
    let flag = if adequacy.fields.is_empty() {
        "--observe ''".to_string()
    } else {
        format!("--observe {}", adequacy.fields.join(","))
    };
    match Plan::build_observing(&verification, &PlanOptions::default(), &fields) {
        Ok(plan) => {
            if let Some(identification) = &plan.identification {
                let steps: usize = plan
                    .tests
                    .iter()
                    .filter(|test| test.kind == TestKind::Identify)
                    .map(|test| test.steps.len())
                    .sum();
                let _ = writeln!(
                    stdout,
                    "default plan: {} of {} transitions could reach a look-alike state unnoticed; zin plan {flag} adds {} ({steps} steps)",
                    identification.unconfirmed,
                    identification.transitions,
                    crate::counted(identification.tests, "test", "tests"),
                );
            }
        }
        Err(error) if error.kind == ErrorKind::Limit => {
            let _ = writeln!(stdout, "note: zin plan {flag} is not feasible: {error}");
        }
        Err(error) => return Err(error),
    }
    if adequacy.adequate() {
        let _ = writeln!(stdout, "every state can be told apart");
    } else {
        let _ = writeln!(
            stdout,
            "{} in {} cannot be told apart",
            crate::counted(adequacy.blind_states, "state", "states"),
            crate::counted(adequacy.blind_groups, "blind group", "blind groups")
        );
    }
    Ok(Output {
        code: u8::from(!adequacy.adequate()),
        stdout,
        stderr: String::new(),
    })
}

fn ok(stdout: String) -> Output {
    Output {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn trace(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &["symmetry"])?;
    let [spec, events] = arguments.positional.as_slice() else {
        return Err(Error::usage(
            "trace needs a specification file and an events file (or - for stdin)",
        ));
    };
    // Replay steps the model directly, so it needs the same proof as check, not the full graph.
    let verification = verify_with(&load_spec(spec)?, verify_options(&arguments)?)?;
    let report = if events == "-" {
        check_trace_reader(
            &verification.model,
            std::io::stdin().lock(),
            MAX_TRACE_BYTES,
        )?
    } else {
        let file = std::fs::File::open(events)
            .map_err(|e| Error::usage(format!("cannot read {events}: {e}")))?;
        check_trace_reader(
            &verification.model,
            std::io::BufReader::new(file),
            MAX_TRACE_BYTES,
        )?
    };
    Ok(ok(format!(
        "ok {}: {} events in {} run(s) conform to the specification\n",
        verification.model.name, report.events, report.runs
    )))
}

fn run_audit(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &[])?;
    let path = arguments.one("specification file")?;
    let audit = audit(&load_spec(path)?)?;
    let mut stdout = String::new();
    let survivors = audit.survivors();
    for mutant in &audit.mutants {
        let (mark, detail) = match &mutant.fate {
            MutantFate::Caught(by) => ("caught  ", by.as_str()),
            MutantFate::Survived => ("SURVIVED", "no property or scenario notices"),
            MutantFate::Equivalent => ("same    ", "reachable behavior is unchanged"),
            MutantFate::Invalid => ("skipped ", "does not type-check"),
            MutantFate::Inconclusive => ("skipped ", "exceeds an exploration limit"),
        };
        let _ = writeln!(
            stdout,
            "{mark} line {}: {} ({detail})",
            mutant.line, mutant.description
        );
        if let Some(witness) = &mutant.witness {
            let outcome = |state: &Option<crate::State>| {
                state.as_ref().map_or_else(
                    || "rejected".to_string(),
                    |state| format!("accepted, {}", crate::display_state(state)),
                )
            };
            for call in &witness.prefix {
                let _ = writeln!(stdout, "  {call}");
            }
            let _ = writeln!(
                stdout,
                "  {} → original {}, mutant {}",
                witness.call,
                outcome(&witness.original),
                outcome(&witness.mutated)
            );
        }
    }
    if audit.truncated {
        let _ = writeln!(
            stdout,
            "note: only the first {} mutants were tried",
            audit.mutants.len()
        );
    }
    let _ = writeln!(stdout, "{}", audit.summary());
    Ok(Output {
        code: u8::from(!survivors.is_empty()),
        stdout,
        stderr: String::new(),
    })
}

fn init(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &[])?;
    let [framework, spec] = arguments.positional.as_slice() else {
        return Err(Error::usage(format!(
            "init needs a framework ({}) and a specification file",
            Framework::NAMES
        )));
    };
    let framework = Framework::parse(framework)?;
    let verification = load(spec)?;
    let file = std::path::Path::new(spec)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(spec);
    Ok(ok(scaffold(&verification.model, file, framework)))
}

fn diagram(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &[])?;
    let (spec, scenario) = match arguments.positional.as_slice() {
        [spec] => (spec, None),
        [spec, scenario] => (spec, Some(scenario)),
        _ => {
            return Err(Error::usage(
                "diagram needs a specification file and optionally a scenario name",
            ));
        }
    };
    let verification = load(spec)?;
    let Some(name) = scenario else {
        return Ok(ok(state_diagram(&verification)));
    };
    let run = verification
        .scenarios
        .iter()
        .find(|run| &run.name == name)
        .ok_or_else(|| Error::usage(format!("no scenario named \"{name}\"")))?;
    Ok(ok(sequence_diagram(&verification, run)))
}

fn dispatch(args: &[String]) -> Result<Output> {
    let Some((command, rest)) = args.split_first() else {
        return Err(Error::usage(USAGE));
    };
    match command.as_str() {
        "check" => check(rest),
        "plan" => plan(rest),
        "trace" => trace(rest),
        "audit" => run_audit(rest),
        "observe" => observe(rest),
        "init" => init(rest),
        "report" => {
            let arguments = Arguments::parse(rest, &[])?;
            Ok(ok(render_markdown(&load(
                arguments.one("specification file")?,
            )?)))
        }
        "diagram" => diagram(rest),
        "--version" | "-V" | "version" => Ok(ok(format!("zin {}\n", env!("CARGO_PKG_VERSION")))),
        "--help" | "-h" | "help" => Ok(ok(format!("{USAGE}\n"))),
        other => Err(Error::usage(format!(
            "unknown command '{other}'\n\n{USAGE}"
        ))),
    }
}

/// Runs the command line. `args` excludes the program name.
#[must_use]
pub fn run(args: &[String]) -> Output {
    match dispatch(args) {
        Ok(output) => output,
        Err(error) => Output {
            code: error.kind.exit_code(),
            stdout: String::new(),
            stderr: if error.kind == ErrorKind::Usage {
                format!("{error}\n")
            } else {
                format!("{}: {error}\n", error.kind.label())
            },
        },
    }
}
