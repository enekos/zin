use crate::ast::{ActionCall, CallArguments, Value};
use crate::display_state;
use crate::json::Json;
use crate::model::{FieldType, Model, resolve_call};
use crate::plan::value_json;
use crate::{Error, ErrorKind, Result};
use std::io::BufRead;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceReport {
    pub events: usize,
    pub runs: usize,
}

fn violation(line: usize, message: String) -> Error {
    Error::of(
        ErrorKind::Violation,
        None,
        format!("trace line {line}: {message}"),
    )
}

fn malformed(line: usize, message: impl AsRef<str>) -> Error {
    Error::usage(format!("trace line {line}: {}", message.as_ref()))
}

fn argument(json: &Json, line: usize) -> Result<Value> {
    match json {
        Json::Int(raw) => Ok(Value::Integer(*raw)),
        Json::Bool(flag) => Ok(Value::Boolean(*flag)),
        Json::Str(name) => Ok(Value::Symbol(name.clone())),
        _ => Err(malformed(
            line,
            "arguments must be integers, booleans, or strings",
        )),
    }
}

fn arguments(event: &Json, line: usize) -> Result<CallArguments> {
    match event.get("args") {
        None | Some(Json::Null) => Ok(CallArguments::Positional(Vec::new())),
        Some(Json::Array(items)) => items
            .iter()
            .map(|item| argument(item, line))
            .collect::<Result<_>>()
            .map(CallArguments::Positional),
        Some(Json::Object(entries)) => entries
            .iter()
            .map(|(name, item)| Ok((name.clone(), argument(item, line)?)))
            .collect::<Result<_>>()
            .map(CallArguments::Named),
        Some(_) => Err(malformed(line, "'args' must be an array or an object")),
    }
}

/// Whether an observed value agrees with the specification's value of type
/// `kind`: maps and records may list a subset of their keys, and a set
/// matches in any order.
fn matches(kind: &FieldType, expected: &Json, observed: &Json) -> bool {
    match (kind, expected, observed) {
        (FieldType::Map { value, .. }, Json::Object(_), Json::Object(entries)) => {
            entries.iter().all(|(key, item)| {
                expected
                    .get(key)
                    .is_some_and(|wanted| matches(value, wanted, item))
            })
        }
        (FieldType::Record(members), Json::Object(_), Json::Object(entries)) => {
            entries.iter().all(|(key, item)| {
                members
                    .iter()
                    .find(|(member, _)| member == key)
                    .zip(expected.get(key))
                    .is_some_and(|((_, kind), wanted)| matches(kind, wanted, item))
            })
        }
        (FieldType::Optional(value), wanted, observed) if *wanted != Json::Null => {
            matches(value, wanted, observed)
        }
        (FieldType::Set { .. }, Json::Array(wanted), Json::Array(items)) => {
            items.len() == wanted.len() && items.iter().all(|item| wanted.contains(item))
        }
        _ => expected == observed,
    }
}

/// Checks a recorded execution, one JSON event per line, against the model.
pub fn check_trace(model: &Model, events: &str) -> Result<TraceReport> {
    check_trace_reader(model, events.as_bytes(), u64::MAX)
}

/// Replays JSONL incrementally. The byte limit applies to the entire input,
/// including blank lines. One extra byte distinguishes EOF from a truncated last event.
pub fn check_trace_reader(model: &Model, reader: impl BufRead, limit: u64) -> Result<TraceReport> {
    let mut reader = reader.take(limit.saturating_add(1));
    let mut consumed = 0u64;
    let mut line = 0;
    let mut text = String::new();
    let mut locals = model.new_locals();
    let mut state = model.initial.clone();
    let mut report = TraceReport { events: 0, runs: 1 };
    loop {
        line += 1;
        text.clear();
        let bytes = reader
            .read_line(&mut text)
            .map_err(|e| malformed(line, e.to_string()))?;
        if bytes == 0 {
            break;
        }
        consumed += bytes as u64;
        if consumed > limit {
            return Err(Error::usage(format!("trace is larger than {limit} bytes")));
        }
        if text.trim().is_empty() {
            continue;
        }
        let event = Json::parse(&text).map_err(|message| malformed(line, message))?;
        if !matches!(event, Json::Object(_)) {
            return Err(malformed(line, "each line must be a JSON object"));
        }
        if event.get("reset") == Some(&Json::Bool(true)) {
            state = model.initial.clone();
            if report.events > 0 {
                report.runs += 1;
            }
            continue;
        }
        let name = event
            .get("action")
            .and_then(Json::as_str)
            .ok_or_else(|| malformed(line, "missing \"action\""))?;
        let accepted = match event.get("outcome").map(Json::as_str) {
            None | Some(Some("accepted")) => true,
            Some(Some("rejected")) => false,
            Some(_) => {
                return Err(malformed(
                    line,
                    "\"outcome\" must be \"accepted\" or \"rejected\"",
                ));
            }
        };
        let call = resolve_call(
            model,
            &ActionCall {
                name: name.to_string(),
                arguments: arguments(&event, line)?,
                line,
                span: crate::ast::Span::default(),
            },
        )
        .map_err(|error| malformed(line, error.message))?;
        let call_text = || model.call_text(call);
        let before = || display_state(&model.decode(&state));
        match model.step(call, &state, &mut locals) {
            Ok(Some(next)) if accepted => state = next,
            Ok(None) if !accepted => {}
            Ok(Some(_)) => {
                return Err(violation(
                    line,
                    format!(
                        "the implementation rejected {}, but the specification allows it in state {}",
                        call_text(),
                        before()
                    ),
                ));
            }
            Ok(None) => {
                return Err(violation(
                    line,
                    format!(
                        "the implementation accepted {}, but the specification rejects it in state {}",
                        call_text(),
                        before()
                    ),
                ));
            }
            Err(fault) => {
                return Err(violation(
                    line,
                    format!(
                        "the specification cannot take {} in state {}: {}",
                        call_text(),
                        before(),
                        fault.message
                    ),
                ));
            }
        }
        report.events += 1;
        if let Some(observed) = event.get("state") {
            let Json::Object(entries) = observed else {
                return Err(malformed(line, "\"state\" must be an object"));
            };
            let expected = model.decode(&state);
            for (field, actual) in entries {
                let wanted = expected
                    .get(field)
                    .map(value_json)
                    .ok_or_else(|| malformed(line, format!("'{field}' is not a state field")))?;
                let kind = model
                    .fields
                    .iter()
                    .find(|info| info.name == *field)
                    .map(|info| &info.kind);
                if !kind.is_some_and(|kind| matches(kind, &wanted, actual)) {
                    return Err(violation(
                        line,
                        format!(
                            "after {}, {field} is {}, but the specification says {}",
                            call_text(),
                            actual.to_compact(),
                            wanted.to_compact()
                        ),
                    ));
                }
            }
        }
    }
    Ok(TraceReport {
        events: report.events,
        runs: report.runs,
    })
}
