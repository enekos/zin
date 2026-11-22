# Changelog

## Unreleased

- Language: `include "relative/path.zin"` loads a specification from several
  files. Inclusion is textual, each file is read once, and paths stay within
  the entry file's directory. Rust: `parse_file`.
- Diagnostics carry the file, line and column the parser recorded for the
  offending token, a stable `code` and, for conflicting declarations, a
  `related` location: `zin check --format json` reports `code`, `span` and
  `related`; Rust has `Error::code`, `source_span` and `related_span`.
  Composition conflict messages no longer embed line numbers.
- `zin plan --component NAME` tests one block of a composition against a
  controllable environment; Rust: `verify_component`. Plans gain `sources`
  and `component`, and a plan's `fingerprint` covers every included file.
- Breaking for Rust callers: AST nodes gain `span` fields and
  `Expr::Name` is a struct variant; `Error` has a private field.
- Runners: `docs/plan.schema.json` defines the plan, and the Python,
  JavaScript and Go runners pass one shared set of contract cases
  (`bindings/fixtures/contract`): staleness over included files, option
  compatibility, malformed calls, observation normalization and the exact
  divergence report. All three accept `max_steps`, `step_budget`,
  `pair_limit` and `component` and compare only the options passed with a
  committed plan, so a plan for `cover="pairs"` or a component is never
  replaced silently. A `Divergence` carries the spec `fingerprint` and a
  pasteable `scenario`. Go no longer drops the error from a shorter replay,
  and every runner rejects a call whose arguments do not fit its action.
- JUnit 5 runner (`bindings/junit`, `io.github.enekos:zin-junit`): plan tests
  become dynamic tests, it passes the shared contract cases, and
  `zin init junit` prints a skeleton that finds its specification on the test
  classpath. Install it from a checkout; it is not on Maven Central yet.
- Model coverage: `pytest --zin-coverage` and the `zin-verify/vitest` entry
  with `zin-verify/vitest-reporter` report the model operations executed,
  planned and eligible, failures and observed fields. `vitest` is an optional
  peer dependency.

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
