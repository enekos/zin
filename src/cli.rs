//! The `zin` command line, as a library function so it can be tested.

use crate::audit::{MutantFate, audit};
use crate::check::{Options, Verification, verify, verify_with};
use crate::json::Json;
use crate::mermaid::{sequence_diagram, state_diagram};
use crate::plan::{Cover, Plan, PlanOptions, state_json};
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
  zin check <spec>... [--format json] [--symmetry on|off]
                                        prove every property over every reachable state
  zin plan <spec> [--out FILE] [--cover transitions|states|actions|scenarios|pairs] [--max-steps N] [--step-budget N] [--pair-limit N]
                                        derive the conformance test plan (JSON); --component NAME isolates a component
  zin refine <abstract> <concrete> [--field abstract=concrete] [--action abstract=concrete] [--max-pairs N]
                                        compare visible state, accepted calls and refusals
  zin trace <spec> <events.jsonl|->     check a recorded execution against the specification
  zin audit <spec>                      mutate the specification; list changes nothing notices
  zin init <framework> <spec>           print a conformance test skeleton
                                        (pytest, unittest, jest, vitest, node, playwright,
                                        testing-library, go, rust, junit, xunit, rspec, phpunit)
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
}

impl Arguments {
    fn parse(args: &[String], valued: &[&str]) -> Result<Self> {
        let mut parsed = Self {
            positional: Vec::new(),
            options: Vec::new(),
        };
        let mut rest = args.iter();
        while let Some(argument) = rest.next() {
            if let Some(name) = argument.strip_prefix("--") {
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

fn load_spec(path: &str) -> Result<Spec> {
    if path == "-" {
        parse(&read(path, MAX_SPEC_BYTES)?)
    } else {
        parse_file(path)
    }
}

fn load(path: &str) -> Result<Verification> {
    verify(&load_spec(path)?)
}

fn load_with(path: &str, options: Options) -> Result<Verification> {
    verify_with(&load_spec(path)?, options)
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
        ("span", error.source_span().map_or(Json::Null, span_json)),
        (
            "related",
            error.related_span().map_or(Json::Null, span_json),
        ),
        ("line", error.line.map_or(Json::Null, Json::count)),
        ("message", Json::str(error.message.clone())),
        ("trace", trace),
    ])
}

fn check_json(path: &str, result: &Result<Verification>) -> Json {
    match result {
        Ok(verification) => Json::object([
            ("file", Json::str(path)),
            ("ok", Json::Bool(true)),
            ("system", Json::str(verification.model.name.clone())),
            ("states", Json::count(verification.graph.state_count())),
            ("calls", Json::count(verification.model.call_count())),
            (
                "attempted_calls",
                Json::Int(
                    i64::try_from(verification.graph.state_count()).expect("bounded state count")
                        * i64::try_from(verification.model.call_count())
                            .expect("bounded call count"),
                ),
            ),
            (
                "relabelings",
                Json::count(
                    verification
                        .model
                        .symmetry
                        .as_ref()
                        .map_or(0, |s| s.relabeling_count()),
                ),
            ),
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
                            let span = verification
                                .model
                                .source_map
                                .as_ref()
                                .and_then(|map| map.span(property.line, None));
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
        ]),
        Err(error) => Json::object([
            ("file", Json::str(path)),
            ("ok", Json::Bool(false)),
            ("error", error_json(error)),
        ]),
    }
}

fn check(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &["format", "symmetry"])?;
    let json = match arguments.option("format") {
        None | Some("text") => false,
        Some("json") => true,
        Some(other) => return Err(Error::usage(format!("unknown format '{other}'"))),
    };
    let options = Options {
        symmetry: match arguments.option("symmetry") {
            None | Some("on") => true,
            Some("off") => false,
            Some(other) => {
                return Err(Error::usage(format!(
                    "--symmetry is on or off, not '{other}'"
                )));
            }
        },
    };
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
        let result = load_with(path, options);
        if let Err(error) = &result {
            output.code = output.code.max(error.kind.exit_code());
        }
        if json {
            results.push(check_json(path, &result));
            continue;
        }
        match result {
            Ok(verification) => {
                let _ = writeln!(output.stdout, "ok {}", verification.summary());
                for note in &verification.notes {
                    let _ = writeln!(output.stdout, "  note: {note}");
                }
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
    options.pair_limit = positive("pair-limit")?;
    let spec = load_spec(arguments.one("specification file")?)?;
    let verification = match arguments.option("component") {
        Some(name) => crate::verify_component(&spec, name)?,
        None => verify(&spec)?,
    };
    let plan = Plan::build(&verification, &options)?;
    let mut text = plan.to_json(&verification).to_compact();
    text.push('\n');
    match arguments.option("out") {
        Some(path) => {
            std::fs::write(path, text)
                .map_err(|error| Error::usage(format!("cannot write {path}: {error}")))?;
            Ok(ok(format!(
                "ok {}\nplan: {} -> {path}\n",
                verification.summary(),
                plan.summary()
            )))
        }
        None => Ok(ok(text)),
    }
}

fn ok(stdout: String) -> Output {
    Output {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

fn trace(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &[])?;
    let [spec, events] = arguments.positional.as_slice() else {
        return Err(Error::usage(
            "trace needs a specification file and an events file (or - for stdin)",
        ));
    };
    let verification = load(spec)?;
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
    let spec = load_spec(path)?;
    let audit = audit(&spec)?;
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
        let location = spec
            .source_map
            .as_ref()
            .and_then(|map| map.span(mutant.line, None))
            .filter(|span| !span.file.is_empty() && !spec.sources.is_empty())
            .map_or_else(
                || format!("line {}", mutant.line),
                |span| format!("{}:{}:{}", span.file, span.line, span.column),
            );
        let _ = writeln!(
            stdout,
            "{mark} {location}: {} ({detail})",
            mutant.description
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
    let path = if framework == Framework::Junit {
        spec
    } else {
        file
    };
    Ok(ok(scaffold(&verification.model, path, framework)))
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

fn refinement(args: &[String]) -> Result<Output> {
    let arguments = Arguments::parse(args, &["field", "action", "max-pairs"])?;
    let [contract, concrete] = arguments.positional.as_slice() else {
        return Err(Error::usage(
            "refine needs an abstract and a concrete specification",
        ));
    };
    if contract == "-" && concrete == "-" {
        return Err(Error::usage("only one specification can come from stdin"));
    }
    let mut options = crate::RefinementOptions::default();
    for (flag, value) in &arguments.options {
        if flag == "max-pairs" {
            options.max_pairs = value
                .parse()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| Error::usage("--max-pairs needs a positive integer"))?;
            continue;
        }
        let (from, to) = value
            .split_once('=')
            .filter(|(a, b)| !a.is_empty() && !b.is_empty())
            .ok_or_else(|| Error::usage(format!("--{flag} needs abstract=concrete")))?;
        let table = if flag == "action" {
            &mut options.actions
        } else {
            &mut options.fields
        };
        if table.insert(from.to_owned(), to.to_owned()).is_some() {
            return Err(Error::usage(format!(
                "duplicate --{flag} mapping for {from}"
            )));
        }
    }
    let checked = crate::refine(&load_spec(contract)?, &load_spec(concrete)?, &options)?;
    Ok(ok(format!(
        "ok {concrete} refines {contract}: {} reachable state pairs\n",
        checked.state_pairs
    )))
}

fn dispatch(args: &[String]) -> Result<Output> {
    let Some((command, rest)) = args.split_first() else {
        return Err(Error::usage(USAGE));
    };
    match command.as_str() {
        "check" => check(rest),
        "refine" => refinement(rest),
        "plan" => plan(rest),
        "trace" => trace(rest),
        "audit" => run_audit(rest),
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
