---
title: Command line
parent: Reference
nav_order: 1
---

# Command line
{: .no_toc }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

```text
zin check <spec>... [--format json] [--symmetry on|off] [--inductive] [--fix]
zin plan <spec> [--out FILE] [--cover transitions|states|actions|scenarios|pairs] [--max-steps N]
         [--step-budget N] [--pair-limit N] [--component NAME] [--observe FIELDS]
zin observe <spec> [--fields a,b] [--component NAME]
zin trace <spec> <events.jsonl|-> [--symmetry on|off]
zin audit <spec>
zin init <framework> <spec>
zin report <spec>
zin diagram <spec> [scenario]
zin explain [code]
zin --version
```

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | ok |
| `1` | the specification or the implementation is wrong: a property fails, a mutant survives, a trace diverges, states are blind |
| `2` | the input could not be used: a syntax or type error, a missing file, a bad flag, a limit |

Every command that takes a specification reads it with its `include` files. `-` in place of a path
reads one self-contained specification from standard input.

## `zin check`

Explores every reachable state of each specification and checks every property and scenario.

```console
$ zin check loan.zin
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
```

| Flag | |
| --- | --- |
| `--format json` | one [`check/1`](../plan-format.md#check1) document for all files: counts, properties with spans, notes, and for failures the error `kind`, `code`, `span`, `related`, `message`, `trace` and `fixes` |
| `--symmetry on\|off` | reduce [interchangeable values](../language/types.md) (default `on`) |
| `--inductive` | also ask whether the `always`/`never` properties are inductive: whether every call from any well-typed state satisfying them keeps them. A non-inductive result is a note, not a failure. See [properties](../language/properties.md) |
| `--fix` | apply each fix a diagnostic is sure of, re-check, and repeat until the specification checks or an error has no single fix |

```console
$ zin check typo.zin
spec: typo.zin:28:18: unknown name mm; declare it or use a quoted enum value
  hint: did you mean `m`?
    28 |   holder becomes m
       |                  ^
  = see: zin explain name.unknown

$ zin check --fix typo.zin
fixed typo.zin:28:18: mm -> m
fixed typo.zin:34:12: renewal -> renewals
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
```

After a successful check, `zin check` also prints notes: actions never enabled in any reachable state,
reads that can find no value and that nothing guards, and why symmetry reduction was not used. See
[diagnostics](../language/diagnostics.md).

## `zin plan`

Checks the specification, then derives the conformance [plan](../plan-format.md#plan1) as one line of
JSON.

```console
$ zin plan loan.zin --out loan.plan.json
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
plan: 4 tests, 52 accepted steps, 64 rejected steps (cover: transitions) -> loan.plan.json
```

| Flag | Default | |
| --- | --- | --- |
| `--out FILE` | standard output | where to write the plan |
| `--cover` | `transitions` | `transitions`: every transition and every refusal in every state; `states`: every state; `actions`: every action accepted and refused once; `scenarios`: only yours; `pairs`: every adjacent pair of calls from every state |
| `--max-steps N` | `200` | soft target for tour length, not counting the setup to the state a tour reached |
| `--step-budget N` | `2000000` (also the maximum) | hard ceiling on every emitted step; a plan that does not fit is a limit error |
| `--pair-limit N` | none | with `--cover pairs`, a deterministic sample of N pairs, repetitions first |
| `--component NAME` | whole system | test one `system` block of a composition, the others becoming its environment |
| `--observe FIELDS` | none | add *identify* tests that confirm each state reached when `observe` returns exactly these comma-separated fields; `''` for an adapter without `observe` |

```console
$ zin plan loan.zin --cover pairs --pair-limit 20 --out loan.pairs.json
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
plan: 4 tests, 37 accepted steps, 41 rejected steps (cover: pairs, 68 of 540 pairs) -> loan.pairs.json
```

The covered count includes every distinct adjacent pair the plan exercises, setup included.

## `zin observe`

Which states a test run can tell apart when `observe` returns the given fields. Without `--fields`,
for an adapter with no `observe`. Exits `1` when some states can never be told apart.

| Flag | |
| --- | --- |
| `--fields a,b` | the fields `observe` returns |
| `--component NAME` | for an adapter of one block of a composition |

See [auditing and observability](../guides/audit-and-observe.md#zin-observe-can-the-tests-see-the-state).

## `zin trace`

Checks a JSON-lines log of a real execution. `-` reads the log from standard input.

```console
$ zin trace loan.zin events.jsonl
ok Library loan: 7 events in 1 run(s) conform to the specification
```

See [checking recorded executions](../guides/traces.md) for the format.

## `zin audit`

Mutates the specification one rule at a time and reports the changes no property or scenario notices.
Exits `1` when a mutant survives. At most 500 mutants per run. See
[auditing](../guides/audit-and-observe.md#zin-audit-is-the-specification-pinned-down).

## `zin init`

Prints a conformance test skeleton with one entry per action, for:

| Framework | Binding |
| --- | --- |
| `pytest`, `unittest` | [Python](../bindings/python.md) |
| `jest`, `vitest`, `node` | [JavaScript](../bindings/javascript.md) (Mocha uses the Jest skeleton) |
| `playwright`, `testing-library` | [Playwright and components](../bindings/playwright.md) |
| `go` | [Go](../bindings/go.md) |
| `rust` | [Rust](../bindings/rust.md) |
| `junit` | [JUnit 5](../bindings/junit.md) |

```console
$ zin init pytest loan.zin > test_loan.py
```

## `zin report`

A Markdown report: the counts, every proven property, the plan's size, and a Mermaid state diagram.

```console
$ zin report loan.zin
# Library loan

Every reachable state was explored: **15 states**, **26 transitions**, 0 terminal.

## Proven properties

- `always holder is nothing implies renewals is 0`
- `always holder is nothing or not waiting has holder`
- `sometimes renewals is 2`
- `whenever waiting is not [] eventually holder is nothing`

## Conformance plan

`zin plan` derives 4 tests, 52 accepted steps, 64 rejected steps (cover: transitions) that together exercise every transition and every refusal.
...
```

## `zin diagram`

A Mermaid state diagram of the reachable graph, or, with a scenario's name, its sequence diagram:

```console
$ zin diagram loan.zin "A reservation blocks renewal"
sequenceDiagram
    participant Client
    participant System as Library loan
    Note over System: holder=nothing, renewals=0, waiting=[]
    Client->>System: borrow(ana)
    Note over System: holder=ana, renewals=0, waiting=[]
    Client->>System: renew
    Note over System: holder=ana, renewals=1, waiting=[]
    Client->>System: reserve(ben)
    Note over System: holder=ana, renewals=1, waiting=[ben]
    Client-xSystem: renew (rejected)
    Note right of System: expect renewals is 1
```

GitHub renders Mermaid in Markdown, so `zin report spec.zin > SPEC.md` is a readable page.

## `zin explain`

Without a code, lists every diagnostic code. With one, prints what it means, a specification that gets
it, the exact output and the fix. The same pages are under [diagnostic codes](../errors/README.md).

```console
$ zin explain
spec                          The specification cannot be read: a syntax error or a rule of the language it breaks.
usage                         The command line or a file it names cannot be used.
model.fault                   Some reachable step leaves a declared range, divides by zero or reads a value that is not there.
violation                     A property does not hold; the counterexample shows how.
limit                         Exploration stopped at a limit before it could decide.
...
```

## Environment

| Variable | Read by | |
| --- | --- | --- |
| `ZIN_BIN` | the Python, JavaScript, Go and JUnit bindings | path of the `zin` binary to run `zin plan` with; otherwise `zin` on `PATH` |

## Limits

Exploration stops with an error, never a partial answer, at the declared state limit
(`explore at most N states`, default 1,000,000) or at 20 million transitions. A specification may have
at most 100,000 distinct calls (action × arguments) and 4,096 scalar slots; nesting depth is limited
to 64; `include` allows 1,024 files, 64 levels deep, 4 MiB in total.
