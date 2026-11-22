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
