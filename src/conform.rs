//! Run a specification's derived test plan against a Rust implementation,
//! in-process, from an ordinary `#[test]`.

use crate::ast::Value;
use crate::check::{Verification, verify};
use crate::model::named_values;
use crate::plan::{Plan, PlanOptions, PlanStep};
use crate::{Error, ErrorKind, Result, State, parse, parse_file};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

pub type Args = BTreeMap<String, Value>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Rejected,
}

impl From<bool> for Outcome {
    fn from(accepted: bool) -> Self {
        if accepted {
            Self::Accepted
        } else {
            Self::Rejected
        }
    }
}

/// A deliberate refusal by the implementation, as opposed to a failure.
///
/// Only `Result<T, Refused>` converts into an [`Outcome`]. Any other error
/// type must say which of its errors are refusals, through
/// [`Outcome::refused_if`], so an I/O timeout can never pass as a rejection:
///
/// ```compile_fail
/// let timeout: Result<(), std::io::Error> = Err(std::io::ErrorKind::TimedOut.into());
/// let _: zin::conform::Outcome = timeout.into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Refused;

impl<T> From<std::result::Result<T, Refused>> for Outcome {
    fn from(result: std::result::Result<T, Refused>) -> Self {
        Self::from(result.is_ok())
    }
}

impl Outcome {
    /// Accepted on `Ok`, rejected on an error that `refused` recognises as a
    /// refusal.
    ///
    /// # Panics
    ///
    /// Panics on any other error: an unexpected failure fails the test as a
    /// crash, never counting as a rejection.
    pub fn refused_if<T, E: fmt::Debug>(
        result: std::result::Result<T, E>,
        refused: impl FnOnce(&E) -> bool,
    ) -> Self {
        match result {
            Ok(_) => Self::Accepted,
            Err(error) if refused(&error) => Self::Rejected,
            Err(error) => panic!("the implementation failed instead of refusing: {error:?}"),
        }
    }
}

/// Connects the implementation under test to the specification's actions.
///
/// `call` must leave the implementation unchanged when it returns
/// [`Outcome::Rejected`]. A panic is a crash, never a rejection; so is an
/// error that is not a refusal (see [`Outcome::refused_if`]).
pub trait Adapter {
    fn call(&mut self, action: &str, args: &Args) -> Outcome;

    /// The implementation's view of any subset of the specification's state
    /// fields. Fields left out are not compared.
    fn observe(&self) -> Option<State> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conformance {
    pub tests: usize,
    pub accepted_steps: usize,
    pub rejected_steps: usize,
}

struct Divergence {
    step: usize,
    message: String,
}

fn compare<A: Adapter>(
    adapter: &A,
    verification: &Verification,
    state: u32,
    after: &str,
) -> std::result::Result<(), String> {
    let Some(observed) = adapter.observe() else {
        return Ok(());
    };
    let expected = verification.state(state);
    for (field, actual) in &observed {
        let Some(wanted) = expected.get(field) else {
            return Err(format!(
                "observe() returned '{field}', which is not a state field of the specification"
            ));
        };
        let kind = verification
            .model
            .fields
            .iter()
            .find(|info| info.name == *field)
            .map(|info| &info.kind);
        if !kind.is_some_and(|kind| crate::model::observation_matches(kind, wanted, actual)) {
            return Err(format!(
                "{after}, {field} is {}, but the specification says {}",
                actual.source(),
                wanted.source()
            ));
        }
    }
    Ok(())
}

fn run<A: Adapter>(
    adapter: &mut A,
    verification: &Verification,
    steps: &[PlanStep],
) -> Option<Divergence> {
    let model = &verification.model;
    if let Err(message) = compare(adapter, verification, 0, "before any action") {
        return Some(Divergence { step: 0, message });
    }
    let mut state: u32 = 0;
    for (index, step) in steps.iter().enumerate() {
        let text = model.call_text(step.call);
        let outcome = adapter.call(
            model.call_action(step.call),
            &named_values(model, step.call),
        );
        let message = match (step.to, outcome) {
            (Some(to), Outcome::Accepted) => {
                state = to;
                compare(adapter, verification, state, &format!("after {text}")).err()
            }
            (None, Outcome::Rejected) => compare(
                adapter,
                verification,
                state,
                &format!("after rejecting {text}"),
            )
            .err(),
            (Some(_), Outcome::Rejected) => Some(format!(
                "the implementation rejected {text}, but the specification allows it"
            )),
            (None, Outcome::Accepted) => Some(format!(
                "the implementation accepted {text}, but the specification rejects it"
            )),
        };
        if let Some(message) = message {
            return Some(Divergence {
                step: index + 1,
                message,
            });
        }
    }
    None
}

/// How many of the latest steps a report lists before eliding earlier ones.
const LISTED_STEPS: usize = 8;

fn listing(verification: &Verification, steps: &[PlanStep]) -> String {
    let skipped = steps.len().saturating_sub(LISTED_STEPS);
    let mut lines = Vec::new();
    if skipped > 0 {
        lines.push(format!("  ... {skipped} earlier steps"));
    }
    for (index, step) in steps.iter().enumerate().skip(skipped) {
        let text = verification.call_text(step.call);
        lines.push(match step.to {
            Some(_) => format!("  {}. {text}", index + 1),
            None => format!("  {}. reject {text}", index + 1),
        });
    }
    lines.join("\n")
}

fn shortest(verification: &Verification, steps: &[PlanStep], failed: usize) -> Vec<PlanStep> {
    let mut state: u32 = 0;
    for step in steps.iter().take(failed.saturating_sub(1)) {
        if let Some(to) = step.to {
            state = to;
        }
    }
    let mut path: Vec<PlanStep> = verification
        .graph
        .path_to(state)
        .into_iter()
        .map(|edge| PlanStep {
            call: edge.call,
            to: Some(edge.to),
        })
        .collect();
    if let Some(step) = failed.checked_sub(1).and_then(|index| steps.get(index)) {
        path.push(*step);
    }
    path
}

/// Runs every test of the plan derived from `spec_source` against fresh
/// implementations produced by `factory`.
pub fn check<A: Adapter>(
    spec_source: &str,
    options: &PlanOptions,
    factory: impl FnMut() -> A,
) -> Result<Conformance> {
    replay(&verify(&parse(spec_source)?)?, options, factory)
}

/// Like [`check`], but reads the specification from `path` and the files it
/// includes, so a multi-file specification can be tested.
pub fn check_file<A: Adapter>(
    path: impl AsRef<Path>,
    options: &PlanOptions,
    factory: impl FnMut() -> A,
) -> Result<Conformance> {
    replay(&verify(&parse_file(path)?)?, options, factory)
}

fn replay<A: Adapter>(
    verification: &Verification,
    options: &PlanOptions,
    mut factory: impl FnMut() -> A,
) -> Result<Conformance> {
    let plan = Plan::build(verification, options)?;
    for test in &plan.tests {
        let Some(divergence) = run(&mut factory(), verification, &test.steps) else {
            continue;
        };
        let taken = test.steps.get(..divergence.step).unwrap_or(&test.steps);
        let mut message = format!(
            "{}: implementation diverges from the specification in test \"{}\" at step {}\n{}\nsteps taken:\n{}",
            verification.model.name,
            test.name,
            divergence.step,
            divergence.message,
            listing(verification, taken)
        );
        message.push_str(&format!(
            "\nspec fingerprint: {}\nscenario \"Replay divergence\"\n",
            verification.model.fingerprint
        ));
        if taken.is_empty() {
            message.push_str("  expect true");
        } else {
            for (index, step) in taken.iter().enumerate() {
                if index > 0 {
                    message.push('\n');
                }
                let prefix = if step.to.is_some() { "  " } else { "  reject " };
                message.push_str(prefix);
                message.push_str(&verification.call_text(step.call));
            }
        }
        let short = shortest(verification, &test.steps, divergence.step);
        if divergence.step > 0 && short.len() < taken.len() {
            match run(&mut factory(), verification, &short) {
                Some(again) if again.step == short.len() && again.message == divergence.message => {
                    let short = short.get(..again.step).unwrap_or(&short);
                    message.push_str(&format!(
                        "\nshortest reproduction:\n{}",
                        listing(verification, short)
                    ));
                }
                _ => message.push_str(
                    "\nthe shortest path to the same specification state does not reproduce the original mismatch",
                ),
            }
        }
        return Err(Error::of(ErrorKind::Violation, None, message));
    }
    Ok(Conformance {
        tests: plan.tests.len(),
        accepted_steps: plan.accepted_steps,
        rejected_steps: plan.rejected_steps,
    })
}

/// Panics with a readable report unless the implementation conforms.
/// Use from a `#[test]` with `include_str!("../spec.zin")`.
///
/// # Panics
///
/// Panics when the specification is invalid, fails its own checks, or the
/// implementation diverges from it.
pub fn assert_conforms<A: Adapter>(spec_source: &str, factory: impl FnMut() -> A) -> Conformance {
    match check(spec_source, &PlanOptions::default(), factory) {
        Ok(conformance) => conformance,
        Err(error) => panic!("{error}"),
    }
}

/// Like [`assert_conforms`], but reads the specification from `path` and the
/// files it includes. Use `concat!(env!("CARGO_MANIFEST_DIR"), "/tests/spec.zin")`
/// for a path that does not depend on the working directory.
///
/// # Panics
///
/// Panics when a file cannot be read, the specification is invalid or fails
/// its own checks, or the implementation diverges from it.
pub fn assert_conforms_file<A: Adapter>(
    path: impl AsRef<Path>,
    factory: impl FnMut() -> A,
) -> Conformance {
    match check_file(path, &PlanOptions::default(), factory) {
        Ok(conformance) => conformance,
        Err(error) => panic!("{error}"),
    }
}
