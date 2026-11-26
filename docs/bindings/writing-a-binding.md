---
title: Writing a binding
parent: Bindings
nav_order: 7
---

# Writing a binding
{: .no_toc }

A binding for a new language or test framework is a plan replayer: a few
hundred lines, with no parser and no model checker. Everything it needs is in
one JSON document, [`plan/1`](../plan-format.md#plan1), written by `zin plan`.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## What you start from

- **The format.** [`plan/1`](../plan-format.md#plan1) describes every key;
  [`docs/plan.schema.json`](https://github.com/enekos/zin/blob/main/docs/plan.schema.json)
  is the one definition of the file, which the shipped runners also read.
- **The contract.** [What a binding must do](../plan-format.md#what-a-binding-must-do)
  fixes loading, replay and reporting, down to the text of the report.
- **The cases.** [`bindings/fixtures/contract`](https://github.com/enekos/zin/tree/main/bindings/fixtures/contract)
  holds executable cases that every runner (Python, JavaScript, Go, JUnit) runs
  in its own suite. A new runner passes them before it ships.
- **A reference.** [`bindings/python/zin/__init__.py`](https://github.com/enekos/zin/blob/main/bindings/python/zin/__init__.py)
  is the shortest complete runner.

## A plan in one paragraph

`states` lists the model states the plan uses (state `0` is initial),
`calls` every call (`action`, positional `args`, display `text`), and
`tests` the sequences to replay. A step is `[call, to]`: perform
`calls[call]`; if `to >= 0` it must be accepted and the model moves to
`states[to]`; if `to` is `-1` it must be rejected and the model stays.
`parents[s]` is `[parent, call]` on a shortest path to `s`, used for the
shortest reproduction.

```json
{"zin": "plan/1", "system": "Delivery", "fingerprint": "aa144eb30ea314b8", "sources": {},
 "actions": [{"name": "mark_paid", "params": []}],
 "states": [{"paid": false, "stage": "received"}, {"paid": true, "stage": "received"}],
 "parents": [null, [0, 0]],
 "calls": [{"action": "mark_paid", "args": [], "text": "mark_paid"}],
 "tests": [{"name": "tour 1", "kind": "tour", "steps": [[0, 1], [0, -1]]}]}
```

(Abridged: a real plan also has `component`, `cover`, `max_steps`,
`step_budget`, `pair_limit`, `defaults`, `fields` and `summary`.)

## Step by step

### 1. Load

Given a specification path and the planning options the caller passed (any of
`cover`, `max_steps`, `step_budget`, `pair_limit`, `component`):

1. **A `.json` path is the plan itself.** Options with it are an error:
   `planning options need a .zin specification`. If the path is
   `<name>.plan.json` and `<name>.zin` exists beside it, the plan must be fresh
   for it.
2. **Otherwise try the explicit plan, else the sibling** `<name>.plan.json`.
   It is usable when:
   - *fresh*: FNV-1a 64 of the specification's bytes equals `fingerprint`.
     With `sources`, hash instead the text made of that hash and a newline,
     then for each source in byte order of `path`: the path, a tab, the current
     hash of that file (relative to the specification), a newline. A missing
     file is not fresh.
   - *compatible*: every option passed equals the plan's recorded value. For
     the sibling, every coverage option left out must also equal the plan's
     `defaults` entry, so your runner holds no defaults of its own. `component`
     must always match.
3. **An explicit plan that is not usable is an error** naming the reason
   (`was derived from a different version of` / `was derived with different
   planning options`) and the regenerating command
   (`zin plan <spec> <flags> --out <plan>`).
4. **Otherwise run `zin plan <spec>`** with exactly the passed options:
   binary from your option, then `ZIN_BIN`, then `PATH`. If it cannot be found,
   the error gives the sibling's reason (if any), then `the zin binary was not
   found` and the regenerating command. A non-zero exit is an error carrying
   the checker's stderr.
5. **Validate** before use: each call names a declared action with exactly
   as many arguments as it has parameters; `states` and `parents` line up;
   every step is in range. Otherwise `the plan is malformed: ...`.

### 2. Register

One native test per entry of `tests`, named by its `name`, so the framework's
filtering, parallelism and reporting apply. `kind` (`scenario`, `witness`,
`tour`, `identify`) does not change how a test runs.

### 3. Replay

For each test:

1. Build a fresh implementation through the user's factory.
2. Compare the observation with `states[0]` ("before any action").
3. For each step, perform the call with arguments mapped to parameter names,
   and classify the result:
   - accepted;
   - rejected, by your language's idiom (an exception type, a sentinel error,
     a `false` return);
   - **anything else is an error**: let it fail the test unchanged. Never count
     an unexpected error as a rejection.
4. Compare with `to`: accepted where the model accepts, rejected where it
   rejects. Then compare the observed fields with the current model state
   (after a rejection, the unchanged state).
5. Release the implementation, also on failure.

Observation rules:

- Observed maps and records may list a subset of keys; every key must exist.
- Types compare strictly: `false` is not `0`, nothing is not `0`.
- An observed list matches an array of the same length element by element.
- An observed native set matches an array with the same elements in any order.
- Normalize native values first: enum members to their names, map keys to
  text, records (dataclasses, structs, Java records) to objects of members.

### 4. Report

A divergence at step *n* (0 when the initial state differs) is reported
exactly as follows:

```text
<system>: implementation diverges from the specification in test "<test>" at step <n>
<what differed>
steps taken:
  1. <call>
  2. reject <call>
spec fingerprint: <fingerprint>
scenario "Replay divergence"
  <call>
  reject <call>
```

- The scenario lists every step taken, or `expect true` when none were.
- Listings longer than eight steps begin with `  ... <k> earlier steps`.
- When *n* > 0, walk `parents` back from the model state before step *n*,
  append step *n*, and if that is shorter, rerun it on a fresh implementation.
  Append one line: `shortest reproduction:` and its listing when it fails at
  its last step with the same message; `the shorter replay raised: <error>`
  when the implementation errored; otherwise `the shortest path to the same
  specification state does not reproduce the original mismatch`.

Expose the scenario and fingerprint on your divergence type, as the other
runners do.

The mismatch messages are fixed strings:

```text
the implementation accepted <call>, but the specification rejects it
the implementation rejected <call>, but the specification allows it
after <call>, <field> is <value>, but the specification says <value>
after rejecting <call>, <field> is <value>, but the specification says <value>
before any action, <field> is <value>, but the specification says <value>
```

## The contract cases

| File | Pins |
| --- | --- |
| `cases.json` `load` | fresh and stale plans, included-file fingerprints, option compatibility, `.json` paths, malformed calls; runs with a binary path that does not exist |
| `cases.json` `replay` | replay and reporting against a scripted adapter: exact report text, errors that must propagate, the number of implementations built |
| `observations.json` | matching observations: partial maps, unknown keys, strict types |
| `native-observations.json` | normalizing native values: `{"$enum": name}` is your enum-like value, `{"$map": [[k, v], ...]}` a map with non-string keys |

Option names in the cases are the plan's keys (`max_steps`); map them to your
language's spelling. The cases' scripted adapter (`initial`, `rules`,
`observe`, `overrides`) is described in the
[fixtures README](https://github.com/enekos/zin/blob/main/bindings/fixtures/contract/README.md).

## Also worth shipping

- A `Recorder` that writes [trace events](../conformance.md#checking-recorded-executions)
  for `zin trace`.
- A coverage report, like `pytest --zin-coverage`: count distinct
  (model state, call) operations attempted out of those planned, where a test's
  operation at a step is the model state before it and the call.
- A skeleton in `zin init` (`src/scaffold.rs`).
