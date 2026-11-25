# Changelog

## Unreleased

- Records: `a record with (status is one of draft, paid, items is an integer
  from 0 to 2)` as a state field, a named type or a map value. Members read
  right to left (`status of orders of c`), literals are `{status: paid, items:
  2}`, and `is` compares whole records. Any path can be assigned, including
  `status of orders of every customer`.
- Nested maps: a map value may be a map. Index one with brackets
  (`paid_on[c][tue]`) and write a whole map as `{mon: false, tue: false}`.
- Optional values: `<type> or nothing`. `nothing` is the absent value; `is
  nothing` and `is w` compare without reading the value, and reading the value
  of nothing (`left of lease`, `asked + 1`) is a model fault. In plans an
  absent value is `null`. **Breaking:** `nothing` is now a reserved word, so
  it can no longer name an enum value or a field.
- Sets: `a set of worker`, written `[ann, ben]` or `[]`. `s has w`, `s with
  w`, `s without w` (also union and difference with another set), `size of
  s`, and quantifiers over a set field's members. Symmetry reduction moves
  set members with the relabeled values. Rust conformance and `zin trace`
  match a set in any order.
- Lists: `a list of at most 3 message`, written `[ping, pong]` or `[]`, of
  any item type. `l with x` appends (a fault when full), `l without x`
  removes every `x`, `l has x`, `length of l`, `first of l`, `last of l` and
  `rest of l` (faults when empty), and quantifiers over a list field's items.
  Examples: `channel.zin` (a FIFO queue) and `recency.zin` (an LRU order).
- Enumeration values with members: `one of queued, running with (attempt is
  an integer from 1 to 3), succeeded`. Write `running with {attempt: 2}`,
  test the case with `job is running`, and read `attempt of job` (a fault
  when the job is not running). In plans such a value is `{"running":
  {"attempt": 2}}`. Example: `retry.zin`.
- `plan/2`: plans whose state holds records, nested maps, optional values,
  sets, lists or enumerations with members say `plan/2`;
  every other plan stays `plan/1`. The Python, JavaScript, Go and JUnit
  runners read both, and compare `null`, lists in order, native sets in any
  order, and records given as dataclasses, Java records or Go structs.
- Rust API: `FieldType::Map` holds its value type as `Box<FieldType>`, and
  `FieldType::width` gives the number of state slots a type occupies. Map
  reads and writes address slots through key steps with a stride, so nested
  types can share one layout. Checking, plans and diagrams are unchanged.
- `zin observe <spec> [--fields a,b]` reports what an adapter observing those
  fields (none: accepted and rejected calls only) lets a test run tell apart:
  states no sequence of calls separates (exit 1, with the fields to observe),
  the longest sequence two look-alike states need, and how many transitions
  of the default plan could land in a look-alike state unnoticed.
- `zin plan --observe a,b` adds `identify` tests so that every transition the
  plan takes is confirmed to reach the specified state under that
  observation. Such plans carry an `observe` key and tests of kind
  `identify`; plans built without the option are unchanged. Rust:
  `Plan::build_observing`, `adequacy`.

## 0.4.0 — 2026-11-22

- Composition: each component's clauses of a shared action resolve names
  against that component's own parameters, so a parameter declared only by
  another component (say `red`) no longer shadows an enum value or changes
  what the checker proves.
- Conformance: Python raises `PlanError` when an action method returns a
  coroutine or generator instead of counting a step whose body never ran.
  Rust no longer converts every `Result` into an `Outcome`: only
  `Result<_, Refused>` converts, and `Outcome::refused_if` names which errors
  are refusals; any other error panics. **Breaking** for adapters that used
  `.into()` on other error types.
- Plans record zin's `defaults`. A sibling `<name>.plan.json` picked
  automatically must use the default for every option the caller leaves out,
  so a committed `--cover scenarios` plan no longer silently replaces default
  coverage. An explicit plan stays the opt-in. All four runners and the
  contract cases follow the new rule; siblings written before this release
  are regenerated.
- `zin trace` verifies the specification the way `check` does (symmetry
  reduction, `--symmetry on|off`) instead of exploring the full graph first.
- Rust: `check_file` and `assert_conforms_file` load a specification with its
  includes; `zin init rust` generates the path-based call.

## 0.3.0 — 2026-11-22

- Checker: model faults inside `some`/`every` over interchangeable values are
  no longer hidden by symmetry reduction. An interval analysis over declared
  domains turns reduction off only when such a quantifier can actually fault,
  and a generated suite compares reduced and full exploration (#20). Check
  JSON reports `calls`, `attempted_calls` and `relabelings`.
- Planning: a hard total replay budget (`--step-budget`), depth-aware tours
  with `--max-steps` as a soft target, and `--cover pairs` (adjacent calls,
  chained into tours; `--pair-limit` samples them) (#17, #19). `Plan::build`
  rejects symmetry-reduced graphs (#20).
- Language: `include "relative/path.zin"` loads a specification from several
  files. Inclusion is textual, each file is read once, and paths stay within
  the entry file's directory. Rust: `parse_file` (#21).
- Diagnostics carry the file, line and column the parser recorded for the
  offending token, a stable `code` and, for conflicting declarations, a
  `related` location: `zin check --format json` reports `code`, `span` and
  `related`; Rust has `Error::code`, `source_span` and `related_span`.
  Composition conflict messages no longer embed line numbers (#21).
- `zin plan --component NAME` tests one block of a composition against a
  controllable environment; Rust: `verify_component`. Plans gain `sources`
  and `component`, and a plan's `fingerprint` covers every included file (#21).
- Trace replay reads JSONL incrementally under a byte limit; audit survivors
  show a shortest distinguishing call sequence; conformance failures verify
  shorter reproductions and print a pasteable scenario (#16).
- Runners: `docs/plan.schema.json` defines the plan, and the Python,
  JavaScript, Go and JUnit runners pass one shared set of contract cases
  (`bindings/fixtures/contract`): staleness over included files, option
  compatibility, malformed calls, observation normalization and the exact
  divergence report. They accept `max_steps`, `step_budget`, `pair_limit` and
  `component` and compare only the options passed with a committed plan. Go
  no longer drops the error from a shorter replay, and every runner rejects a
  call whose arguments do not fit its action (#22).
- Model coverage: `pytest --zin-coverage` and the `zin-verify/vitest` entry
  with `zin-verify/vitest-reporter` report the model operations executed,
  planned and eligible, failures and observed fields. `vitest` is an optional
  peer dependency (#22).
- JUnit 5 runner (`bindings/junit`, `io.github.enekos:zin-junit`): plan tests
  become dynamic tests and `zin init junit` prints a skeleton that finds its
  specification on the test classpath. Install it from a checkout; it is not
  on Maven Central yet (#23).
- Packaging: the crate, wheel and npm tarball are installed into clean
  consumers in CI, and every npm entry point must load with only declared
  dependencies.
- Minimum supported Rust is now 1.88 (let chains). 0.2.0 declared 1.85 but did
  not build on it.
- Breaking for Rust callers:
  - `Action::fair: bool` is now `Action::fairness: Fairness` (`Unfair`, `Weak`
    or `Strong`).
  - `PlanOptions`, `Plan`, `Cover` and `TestKind` are `#[non_exhaustive]`:
    start from `PlanOptions::default()` or `PlanOptions::cover(..)` and assign
    fields.
  - AST nodes gain `span` fields, `Expr::Name` is a struct variant, and
    `Error` has a private field.

## 0.2.0 — 2026-11-18

First published release. The crate is `zin-spec` (crates.io `zin` belongs to an
unrelated project); the binary and the library are still `zin`.

- Language: maps, quantifiers, `always`/`never`/`sometimes`, `eventually`,
  `whenever … eventually`, `always possible`, `fair action`, `terminal when`.
- Tools: `check`, `plan`, `trace`, `audit`, `init`, `report`, `diagram`.
- Conformance: `zin plan` derives tests that take every transition and try
  every refusal; bindings replay them in pytest/unittest, Jest (CommonJS and
  ESM), Vitest, node:test, Mocha, `go test` and `cargo test`. A plan
  fingerprint refuses a stale committed plan.
- Packages: `zin-spec` (crates.io), `zin-verify` (PyPI, npm), the Go module,
  and prebuilt binaries on GitHub releases. MIT license.
