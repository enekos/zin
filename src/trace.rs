use crate::ast::{ActionCall, CallArguments, Value};
use crate::display_state;
use crate::json::Json;
use crate::model::{Model, resolve_call};
use crate::plan::value_json;
use crate::{Error, ErrorKind, Result};

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

fn matches(expected: &Json, observed: &Json) -> bool {
    match (expected, observed) {
        (Json::Object(_), Json::Object(entries)) => entries.iter().all(|(key, value)| {
            expected
                .get(key)
                .is_some_and(|wanted| matches(wanted, value))
        }),
        _ => expected == observed,
    }
}

/// Checks a recorded execution, one JSON event per line, against the model.
pub fn check_trace(model: &Model, events: &str) -> Result<TraceReport> {
    let mut locals = model.new_locals();
    let mut state = model.initial.clone();
    let mut report = TraceReport { events: 0, runs: 1 };
    for (index, text) in events.lines().enumerate() {
        let line = index + 1;
        if text.trim().is_empty() {
            continue;
        }
        let event = Json::parse(text).map_err(|message| malformed(line, message))?;
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
            },
        )
        .map_err(|error| malformed(line, error.message))?;
        let text = model.call_text(call);
        let before = display_state(&model.decode(&state));
        match model.step(call, &state, &mut locals) {
            Ok(Some(next)) if accepted => state = next,
            Ok(None) if !accepted => {}
            Ok(Some(_)) => {
                return Err(violation(
                    line,
                    format!(
                        "the implementation rejected {text}, but the specification allows it in state {before}"
                    ),
                ));
            }
            Ok(None) => {
                return Err(violation(
                    line,
                    format!(
                        "the implementation accepted {text}, but the specification rejects it in state {before}"
                    ),
                ));
            }
            Err(fault) => {
                return Err(violation(
                    line,
                    format!(
                        "the specification cannot take {text} in state {before}: {}",
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
                if !matches(&wanted, actual) {
                    return Err(violation(
                        line,
                        format!(
                            "after {text}, {field} is {}, but the specification says {}",
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
