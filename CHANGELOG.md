# Changelog

## Unreleased

- Fix: a map literal keyed by an interchangeable type could give its keys
  different values (`load becomes {ann: 0, ben: 0, cid: 2}`), which singles
  values out. Symmetry reduction then explored the wrong representatives:
  `zin check` could pass a specification that `--symmetry off` rejects, or
  print a counterexample that was not a real run. Such literals are now a
  specification error, in `initially`, actions and properties alike.
- A counterexample found on symmetry representatives that does not replay as
  a real run is no longer printed. The check is repeated on the full state
  space, with a note.

## 0.6.0 — 2026-11-26

- `zin check --inductive` asks whether the `always` and `never` properties
  together are an inductive invariant: whether every call from any
  well-typed state where they hold, reachable or not, leads to a state where
  they hold again. When they are not, it shows the first counterexample to
  induction (a state, a call and the property it breaks or the fault it
  raises), which points at the `always` that is missing. The result is
  informational: exit code 0 either way. `check/1` gains an `inductive`
  object. Rust: `check_induction`, `Induction`.
- Suggested fixes. An unknown name suggests the closest declared name of a
  kind that fits where it stands, and the text output shows the corrected
  line. This covers fields, maps, record members, parameters, enum values
  (quoted or not), `true`/`false`/`nothing`, named types, map keys in
  literals, and actions and parameters in scenarios. A missing `include`
  suggests the closest `.zin` file next to it. `zin check --fix` applies
  every suggestion that is the only one for its error. `check/1` errors gain
  a `fixes` list; Rust has `Error::fixes`, `Fix` and `Error::report`.
- `zin explain <code>` explains every diagnostic code with a failing
  specification, its exact output and a fix; `zin explain` lists the codes.
  The pages are `docs/errors/<code>.md`, and the test suite runs each
  example. Text output of an error with a code finer than its kind ends with
  `= see: zin explain <code>`. Rust: `explanation`, `explanations`.
- `read or else default`: the read, or the default where reading it finds no
  value: an optional that is nothing (`left of lease or else 0`), a case
  without the member (`attempt of job or else 0`) or an empty list (`first of
  queue or else ping`, `rest of queue or else []`). It binds more tightly than
  arithmetic, the default is evaluated only when used, and only absence falls
  back: overflow and division by zero still fault. A read that can never be
  absent is an error. Symmetry reduction stays on for quantifiers whose reads
  have defaults.
- `zin check` notes each read that can find no value and that no earlier
  condition guards (`lease is not nothing`, `job is running`, `queue is not
  []`, `length of queue > 0`, ...), with the guard and the default to write.
  `check/1` lists them under `unguarded_reads` with the code
  `read.unguarded`; Rust: `Verification::unguarded`.
- Rust API: `ast::Expr::OrElse`.
- One plan format, `plan/1`, for every specification. It carries records,
  nested maps, optional values, sets, lists and enumerations with members, and
  always records `sources`, `component`, the planning options and their
  `defaults`. Every runner accepts exactly `plan/1`, and the schema requires
  every key `zin plan` writes. Earlier releases are not listed here; their
  history lives in the git tags.
