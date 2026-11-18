# Changelog

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
