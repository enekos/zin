---
title: Bindings
nav_order: 4
has_children: true
---

# Bindings
{: .no_toc }

A binding runs a specification's derived test plan inside a test framework you
already use. You write one **adapter** that connects the specification's
actions to your code; the binding registers one native test per scenario and
tour, replays every step, and reports the first place where your code and the
specification disagree.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## The adapter answers three questions

Every binding asks the same three things, in its own idiom.

1. **How do I perform each action?** One function or method per action, or one
   `call(action, args)` that dispatches on the name. Arguments arrive by
   parameter name.
2. **How does the implementation say no?** A rejection is a normal outcome:
   the specification says some calls must be refused in some states, and the
   test checks that they are. Anything else that goes wrong (an unexpected
   exception, an error that is not a refusal) fails the test as an error. It is
   never silently counted as a rejection.
3. **What state can be seen?** Optional. `observe` returns any subset of the
   specification's state fields; only the returned fields are compared, after
   every step, and after a rejection the state must be unchanged.

Without `observe`, a wrong state is still caught once it changes which calls
are accepted, only later and with a less direct message.
[`zin observe`](../conformance.md#what-observe-can-tell-apart) tells you how
much later, and whether ever.

Each plan test builds a **fresh implementation** through your factory or
`setup`, so tests never share state. When a test fails, the binding builds one
more implementation and replays the shortest path to the failing model state,
to report a shorter reproduction. Whatever your factory starts (a server, a
page, a database row) must therefore start from scratch, and your teardown
must undo it.

## At a glance

| | Python | JavaScript / TypeScript | Playwright | Go | Rust | JUnit 5 |
| --- | --- | --- | --- | --- | --- | --- |
| Install | `pip install` from a checkout | `npm i -D zin-verify` | `npm i -D zin-verify @playwright/test` | `go get github.com/enekos/zin/bindings/go` | `zin-spec` as a dev-dependency | `mvn install` from a checkout |
| Register | `@zin.verify("x.zin")` on a class | `verify(spec, factory, opts)` | `verify(spec, factory, { test })` from `zin-verify/playwright` | `zin.Verify(t, "x.zin", factory)` | `assert_conforms_file(path, factory)` | `Zin.load(spec).tests(factory)` in a `@TestFactory` |
| One test per | `test_NNN_...` method | `test(...)` | Playwright test, one `test.step` per step | `t.Run` subtest | (one `#[test]`, all plan tests in it) | `DynamicTest` |
| Rejection | raise `zin.Rejected` or a type in `rejects`, or return `False` | throw `Rejected` or an error for which `rejects(e)` is true, or return `false` | as JS; `click`/`fill` throw `Rejected` for controls a user could not use | return an error wrapping `zin.ErrRejected` | return `Outcome::Rejected` (`false`, `Err(Refused)` or `refused_if`) | return `false` |
| Observe | `observe(self)` → `dict` | `observe()` → object | `observe()` → object, retried for `observeTimeout` | implement `zin.Observer` | `fn observe(&self) -> Option<State>` | `observe()` → `Map`, or `null` |
| Setup / teardown | `setup()`, `teardown()` | factory; `teardown()` | factory with fixtures; `teardown()` | factory | factory; `Drop` | constructor; `close()` |
| Async | no: methods are synchronous | everything may be async; `settle()` | everything is async | no | no | no |
| `zin init` | `pytest`, `unittest` | `jest`, `vitest`, `node`, `testing-library` | `playwright` | `go` | `rust` | `junit` |

`zin init <framework> <spec>` prints a skeleton with one stub per action and
the state fields listed in a comment. Mocha has no skeleton of its own; use
`jest` (it relies on the same globals).

## How values arrive and how they are compared

Arguments are integers, booleans or enum values: action parameters cannot be
compound. Observed state may be any type the specification has.

| Specification | In a plan | Python | JavaScript | Go | Rust | Java |
| --- | --- | --- | --- | --- | --- | --- |
| integer | number | `int` | `number` | `int64` (`args.Int` gives `int`) | `Value::Integer(i64)` | `Long` |
| boolean | boolean | `bool` | `boolean` | `bool` | `Value::Boolean` | `Boolean` |
| enum value | string | `str` | `string` | `string` | `Value::Symbol` | `String` |
| map | object keyed by the key's text | `dict` | object or `Map` | `map[K]V` | `Value::Map` | `Map` |
| record | object keyed by member | `dict`, dataclass, named tuple | object | struct or map | `Value::Map` | record or `Map` |
| optional | value or `null` | value or `None` | value, `null` or `undefined` | value, `nil` or nil pointer | value or `Value::Nothing` | value, `null` or empty `Optional` |
| set | array in declared order | `set` / `frozenset` (any order) | `Set` (any order) | `zin.Set` (any order) | `Value::List` (any order) | `Set` (any order) |
| list | array | `list` / `tuple` | array | slice | `Value::List` | `List` |
| case with members | `{"case": {members}}` | `{"case": {...}}` | `{ case: {...} }` | `map[string]any` | `Value::Tagged` | `Map` |

Native values are normalized before comparing:

- **Enums.** A Python `enum.Enum` member becomes its value if that is a string,
  else its name. A Go `fmt.Stringer` becomes its `String()`, and a named string
  type its string. A Java enum constant becomes its `toString()`.
- **Records.** A Python dataclass or named tuple, a Go struct (exported fields,
  named by a `zin` tag, else a `json` tag, else the field name with a
  lower-case first letter) and a Java record become an object of their members.
- **Map keys** are compared as text: `true`, `3`, `ana`.
- **Sets.** An observed native set matches the plan's array in any order. An
  ordinary list or array observed for a set field is compared in order, in the
  order the element type declares its values, so prefer the native set.

Types compare strictly: `false` is not `0`, nothing is not `0`, and in Python
`True` is not `1`. Observed maps and records may list a subset of keys, but
every key must exist in the specification.

## Where the plan comes from

The Python, JavaScript, Go and JUnit bindings look, in order, for:

1. **An explicit plan file** you pass (`plan=`, `{ plan }`, `zin.WithPlan`,
   `Options.plan`).
2. **A sibling** `<spec>.plan.json` next to the specification.
3. **The `zin` binary** (`ZIN_BIN`, then `PATH`), run as `zin plan <spec>` with
   exactly the planning options you passed.

A committed plan is used only when it is:

- **Fresh**: its fingerprint matches the specification on disk and every file
  it `include`s. Editing any of them makes it stale.
- **Compatible**: every planning option you passed equals the one the plan was
  derived with. A *sibling* must also have been derived with zin's default for
  every option you leave out, so a committed `--cover states` plan is never
  picked up silently by a suite that expects full coverage. Naming a plan
  explicitly is the opt-in to its options.

An explicit plan that is stale or incompatible is an error that prints the
command that regenerates it. A sibling that is not usable falls through to the
binary; if there is no binary, the error says why the sibling was skipped:

```text
loan.plan.json was derived from a different version of loan.zin; the zin binary was not found. Install zin-spec, set ZIN_BIN, or regenerate the plan: zin plan loan.zin --out loan.plan.json
```

You can also pass a `.plan.json` path *as the specification*: the binding then
replays that plan, after checking it is fresh for the `.zin` beside it.
Planning options with a plan path are an error.

The Rust binding is different: it is the checker itself, so it derives the
plan in-process and needs neither a binary nor a plan file.

**Derive at test time** when every developer machine and CI runner has `zin`:
the specification is re-proved on each run, and a broken specification fails
the tests with the checker's counterexample. **Commit the plan** when test
machines should need nothing but the binding:

```console
$ zin plan loan.zin --out loan.plan.json
ok Library loan: 15 states, 26 transitions, 3 properties, 2 scenarios
plan: 4 tests, 52 accepted steps, 64 rejected steps (cover: transitions) -> loan.plan.json
```

## Planning options

| `zin plan` flag | Python | JavaScript | Go | JUnit | Rust |
| --- | --- | --- | --- | --- | --- |
| `--cover` | `cover="states"` | `cover: "states"` | `zin.WithCover("states")` | `.cover("states")` | `PlanOptions::cover(Cover::States)` |
| `--max-steps` | `max_steps=50` | `maxSteps: 50` | `zin.WithMaxSteps(50)` | `.maxSteps(50)` | `max_steps` field |
| `--step-budget` | `step_budget=` | `stepBudget:` | `zin.WithStepBudget()` | `.stepBudget()` | `step_budget` field |
| `--pair-limit` | `pair_limit=` | `pairLimit:` | `zin.WithPairLimit()` | `.pairLimit()` | `Cover::Pairs { limit }` |
| `--component` | `component="Client"` | `component: "Client"` | `zin.WithComponent("Client")` | `.component("Client")` | `verify_component` + `Plan::build` |
| (plan file) | `plan="x.plan.json"` | `plan:` | `zin.WithPlan()` | `.plan(Path)` | — |
| (binary) | `bin="/path/zin"` | `bin:` | `zin.WithBinary()` | `.binary()` | — |

`max_steps`, `step_budget` and `pair_limit` must be positive integers.

### Choosing coverage

| Cover | The plan contains | Use it for |
| --- | --- | --- |
| `transitions` (default) | every transition, and every rejected call in every state | the whole behavior of the model; the default for unit-speed implementations |
| `states` | a visit to every reachable state | pull requests against slow implementations |
| `actions` | every action accepted once and, where possible, rejected once | a smoke test |
| `scenarios` | only the scenarios you wrote | early adoption, before the model is complete |
| `pairs` | every pair of adjacent calls from every reachable state | implementations that keep history the model does not (`--pair-limit` samples) |

A plan has roughly `states × calls` steps. When that is too many for a slow
implementation, shrink the model's ranges for testing, use `states`, or run
`transitions` nightly. See [coverage](../conformance.md#coverage).

## Anatomy of a divergence

Every binding reports a failure in the same text format. This is the
[Library loan](python.md#a-complete-example) implementation from the Python
page, with a `renew` that forgets to check for reservations (the pasted
scenario, which lists all 27 steps, is abridged here):

```text
Library loan: implementation diverges from the specification in test "tour 1" at step 27
the implementation accepted renew, but the specification rejects it
steps taken:
  ... 19 earlier steps
  20. give_back
  21. borrow(ben)
  22. give_back
  23. borrow(ben)
  24. reserve(ana)
  25. reject borrow(ana)
  26. reject borrow(ben)
  27. reject renew
spec fingerprint: 09d83aa6c856ea87
scenario "Replay divergence"
  reject renew
  reject reserve(ben)
  borrow(ben)
  (... all 27 steps ...)
  reserve(ana)
  reject borrow(ana)
  reject borrow(ben)
  reject renew
shortest reproduction:
  1. borrow(ben)
  2. reserve(ana)
  3. reject renew
```

- **The header** names the system, the plan test, and the step number (0 means
  the initial state already differed).
- **What differed**: one of
  - `the implementation accepted <call>, but the specification rejects it`
  - `the implementation rejected <call>, but the specification allows it`
  - `after <call>, <field> is <value>, but the specification says <value>`
  - `after rejecting <call>, ...` when a refusal changed the state
  - `before any action, ...` when the initial state is wrong.
- **Steps taken**: the last eight, with `reject` marking calls the
  specification refuses.
- **Spec fingerprint**: which version of the specification the plan came
  from.
- **The scenario**: every step taken, as a Zin `scenario` you can paste into
  the specification to keep the case. Every binding's `Divergence` also
  carries it as a property (`scenario`, `Scenario`, `scenario()`).
- **The last line** is one of:
  - `shortest reproduction:` and a shorter listing, rerun on a fresh
    implementation and failing at its last step with the same message;
  - `the shorter replay raised: <error>`, when the implementation errored on
    the shorter path;
  - `the shortest path to the same specification state does not reproduce the
    original mismatch`: the failure depends on history the specification does
    not model.

An error that is neither an acceptance nor a recognized rejection is not a
divergence: it propagates unchanged and fails the test with your stack trace.

## Pages

- [Python: pytest and unittest](python.md)
- [JavaScript and TypeScript](javascript.md): Jest, Vitest, `node:test`, Mocha
- [Playwright and components](playwright.md): a page in a browser, or a component in jsdom
- [Go](go.md)
- [Rust](rust.md)
- [JUnit 5](junit.md)
- [Writing a binding](writing-a-binding.md) for another language
