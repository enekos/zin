# Formats

Everything a binding needs is in one JSON document, so a binding for a new
language or framework is a few hundred lines with no parser and no model
checker. The three shipped bindings are reference implementations
(`bindings/python/zin/__init__.py` is the shortest).

## `plan/1`

Written by `zin plan`. Compact JSON, one line.

```json
{
  "zin": "plan/1",
  "system": "Delivery",
  "fingerprint": "aa144eb30ea314b8",
  "sources": {},
  "component": null,
  "cover": "transitions",
  "max_steps": 200,
  "step_budget": 2000000,
  "pair_limit": null,
  "fields": [
    {"name": "paid", "type": {"kind": "boolean"}},
    {"name": "stage", "type": {"kind": "enum", "values": ["received", "packed"]}}
  ],
  "actions": [
    {"name": "mark_paid", "params": []},
    {"name": "deposit", "params": [{"name": "amount", "type": {"kind": "integer", "min": 1, "max": 5}}]}
  ],
  "states": [{"paid": false, "stage": "received"}, {"paid": true, "stage": "received"}],
  "parents": [null, [0, 0]],
  "calls": [{"action": "mark_paid", "args": [], "text": "mark_paid"}],
  "tests": [
    {"name": "scenario: A paid order leaves the building", "kind": "scenario", "steps": [[0, 1]]},
    {"name": "tour 1", "kind": "tour", "steps": [[0, 1], [0, -1]]}
  ],
  "summary": {"states": 6, "transitions": 6, "refusals": 6, "eligible_pairs": null, "covered_pairs": null, "tests": 6, "accepted_steps": 13, "rejected_steps": 18}
}
```

| Key | Meaning |
| --- | --- |
| `fingerprint` | FNV-1a 64 of the specification file's bytes, 16 lowercase hex digits. A binding compares it with the specification on disk to detect a stale plan. For a specification with `include` directives it covers every file: FNV-1a 64 of the entry file's fingerprint and a newline, followed by `path`, a tab, the file's fingerprint and a newline for each entry of `sources` in order. |
| `sources` | included files, transitively, by path relative to the entry file's directory with `/` separators, each with the FNV-1a 64 of its bytes. Empty for a single file; may be absent in plans from older versions. |
| `component` | the composition block the plan tests in isolation (`zin plan --component`), or `null` for the whole system. May be absent in plans from older versions. |
| `cover` | the coverage goal the plan was derived for: `transitions`, `states`, `actions`, `scenarios` or `pairs` |
| `max_steps`, `step_budget`, `pair_limit` | the soft tour target, the hard total step ceiling and the pair sample size (null unless a sampled `pairs` plan) the plan was derived with. Absent from plans written before 0.3. |
| `observe` | only in plans from `zin plan --observe`: the fields the adapter's `observe` is taken to return, in declaration order (empty for an adapter without `observe`). The plan's `identify` tests rule out states that look alike under these fields. |
| `summary` | full-model `states`, `transitions` and `refusals`, plus the emitted `tests`, `accepted_steps` and `rejected_steps`. `eligible_pairs` and `covered_pairs` are numbers for `pairs` plans and null otherwise. |
| `fields` | the state variables. Types: `integer` (`min`, `max`), `boolean`, `enum` (`values`), `map` (`keys` as strings, `value` type) |
| `actions` | actions and their parameters, in declaration order |
| `states` | the model states the plan refers to, as objects. Integers are JSON numbers, booleans JSON booleans, enum values strings, maps objects keyed by the key's text. State `0` is the initial state. Only states the plan uses are listed. |
| `parents` | for each state, `[parent state, call]` on a shortest path from the initial state; `null` for state 0. Used to build the shortest reproduction. |
| `calls` | every call the plan uses: action name, positional `args` (same order as the action's `params`), and display `text` |
| `tests` | each test starts from a fresh implementation in the initial state. A step is `[call, to]`: perform `calls[call]`; if `to >= 0` it must be accepted and the model moves to `states[to]`; if `to` is `-1` it must be rejected and the model state is unchanged. `kind` is `scenario`, `witness`, `tour` or `identify` and does not change how a test is run. |

All integers fit in a JavaScript safe integer; `zin plan` refuses a
specification whose values do not.

### What a binding must do

[`plan.schema.json`](plan.schema.json) is the one definition of the file. The
Python, JavaScript, Go and JUnit runners all follow the contract below, and each runs
the same executable cases in
[`bindings/fixtures/contract`](../bindings/fixtures/contract/README.md). A new
runner passes those cases before it ships.

**Loading.** Given a specification and the planning options the caller passed
(any of `cover`, `max_steps`, `step_budget`, `pair_limit`, `component`):

1. A path ending in `.json` is the plan itself. Passing options with it is an
   error ("planning options need a .zin specification"). If the path is
   `<name>.plan.json` and `<name>.zin` exists beside it, the plan must be fresh
   for that specification.
2. Otherwise use the explicit plan file, or else a sibling `<name>.plan.json`
   if one exists. It is usable when it is **fresh** and **compatible**:
   - *Fresh:* hash the specification's bytes with FNV-1a 64. With no `sources`,
     that is the fingerprint. With `sources`, hash the text made of that hash
     and a newline, then `path`, a tab, the current hash of the file at that
     path (relative to the specification) and a newline for each source in
     byte order of `path`. A missing file is not fresh.
   - *Compatible:* every option the caller passed equals the plan's recorded
     value (an older plan without the key is not compatible). For the sibling,
     every coverage option left out (`cover`, `max_steps`, `step_budget`,
     `pair_limit`) must equal the plan's recorded `defaults` entry, so runners
     hold no defaults and a plan with weaker coverage is never chosen
     silently; a sibling without `defaults` (written before 0.3.1) is not
     compatible then. An explicit plan compares only the options passed:
     naming it is the opt-in to its coverage. `component` is identity: the
     plan's (null when absent) must equal the requested one.
3. An explicit plan that is not usable is an error naming the reason ("was
   derived from a different version of" or "was derived with different
   planning options") and the `zin plan` command that regenerates it.
4. Otherwise run `zin plan <spec>` with exactly the passed options. If the
   binary cannot be found, the error gives the sibling's reason, if any, then
   "the zin binary was not found" and the regenerating command.
5. Validate every plan before use: each call names a declared action with
   exactly as many arguments as it has parameters, `states` and `parents` line
   up, and every step is in range. A violation is an error starting "the plan
   is malformed".

**Replaying.** For each test build a fresh implementation, then for each step
perform the call, classify the result as accepted or rejected, compare with
`to`, and compare any observed fields with the current model state (after a
rejection: the unchanged state). Observed maps may list a subset of the keys,
but every key must exist; types compare strictly (`false` is not `0`).
Native values are normalized first: enum members to their name, map keys to
their text. Anything that is neither an acceptance nor a recognized rejection
fails the test with that error.

**Reporting.** A divergence at step *n* (0 when the initial state differs) is
reported exactly as follows.

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

The scenario lists the steps taken, or `expect true` when none were. Lists
longer than eight steps begin with `  ... <k> earlier steps`. When *n* > 0,
walk `parents` back from the model state before step *n* to get the shortest
call sequence to it and append step *n*. If that is shorter, rerun it on a
fresh implementation and append one line:

- `shortest reproduction:` and its listing, when it fails at its last step
  with the same message;
- `the shorter replay raised: <error>`, when the implementation raised an
  error (it is reported, never swallowed);
- otherwise `the shortest path to the same specification state does not
  reproduce the original mismatch`.

## `check/1`

Written by `zin check --format json`.

```json
{"zin": "check/1", "results": [
  {"file": "job.zin", "ok": true, "system": "Background job", "states": 10, "calls": 3,
   "attempted_calls": 30, "relabelings": 0, "transitions": 9, "up_to_symmetry": false, "terminal_states": 4,
   "properties": [{"property": "eventually ...", "line": 22,
     "span": {"file": "job.zin", "line": 22, "column": 1, "end_column": 41}}],
   "scenarios": ["Third time lucky"], "notes": []},
  {"file": "stuck.zin", "ok": false, "error": {
     "kind": "violation", "code": "violation", "line": 13,
     "span": {"file": "stuck.zin", "line": 13, "column": 1, "end_column": 30}, "related": null,
     "message": "...",
     "trace": {"initial": {"status": "queued"},
               "steps": [{"call": "start", "state": {"status": "running"}}],
               "loop_from": 0}}}
]}
```

`kind` is `spec` (does not parse or type-check), `model` (an action leaves a
declared range or divides by zero), `violation`, `limit` or `usage`.
`loop_from` is `null` for a finite counterexample; otherwise the steps from
that index on repeat forever. `up_to_symmetry` is `true` when `states` and
`transitions` count representatives of interchangeable values.
`attempted_calls` is `states` times `calls`, the work exploration did, and
`relabelings` is the size of the relabeling tables it built (0 without
reduction). These are added fields; readers of `check/1` should ignore fields
they do not know.

`code` is a stable identifier for the diagnostic: `name.unknown`,
`type.mismatch`, `component.field_conflict`, `component.type_conflict`,
`component.parameter_conflict`, `component.duplicate_action` and
`component.foreign_field`, or the `kind` itself (`model.fault` for a model
fault) when no finer code applies. New codes may be added. `span` is the token
the parser recorded for the problem, or the whole statement when no token
applies; `related` is a second location, such as the first of two conflicting
declarations. Both are `null` when the error has no location. A span's `file`
is the file that holds the text (empty for standard input); `line` is a line of
that file, and so is the top-level `line`. Columns count Unicode scalar values
from one and `end_column` is exclusive. Property `line` and `span` follow the
same rules.

## Trace events

Read by `zin trace`; see [conformance testing](conformance.md#checking-recorded-executions).

CLI plans may include `action_locations`, keyed by action name. Each entry gives the declaration file relative to the source specification directory and its one-based line and column. Playwright uses these locations for steps and divergence reports.

With `zin plan --restore`, the file declares `plan/2` so older runners reject
it before replay. A test's optional `from` indexes the canonical state to seed
(default zero, the ordinary initial state). The JS runner calls
`adapter.restore(state)` before the first observation of a nonzero start.
Scenarios, witnesses and pair tours retain their normal setup. Other tours may
start at any outstanding obligation. With `--observe` the planner also checks
the restored start against behaviorally distinguishable look-alikes.
