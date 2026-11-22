# Changelog

## 0.3.0 — unreleased

- Checker and language: strong fairness, interchangeable enums, synchronized components, multi-file source tracking and bounded refinement. Full-graph fallback preserves model faults in quantified expressions.
- Planning: adjacent-call coverage, explicit sampling, a hard total replay budget and depth-aware tours. Concrete plans require full verification. `states` guarantees state visits; `transitions` covers refusals in every state. `scenarios` emits only authored cases.
- Bindings: browser and component adapters, coverage reporting, opt-in field observation and native JUnit, xUnit, RSpec and PHPUnit consumers. Saved plans include generation options and imported source fingerprints.
- Rust API migration: `Action::fair` is now `Action::fairness` (Unfair, Weak or Strong). Use `..PlanOptions::default()` when selecting coverage or a tour target. Minimum supported Rust is 1.88.
- Audit survivors include a shortest distinguishing call sequence. Trace replay reads JSONL incrementally.

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
