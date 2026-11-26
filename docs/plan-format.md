# Formats

Everything a binding needs is in one JSON document, so a binding for a new
language or framework is a few hundred lines with no parser and no model
checker. The shipped bindings are reference implementations
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
| `sources` | included files, transitively, by path relative to the entry file's directory with `/` separators, each with the FNV-1a 64 of its bytes. Empty for a single file. |
| `component` | the composition block the plan tests in isolation (`zin plan --component`), or `null` for the whole system. |
| `cover` | the coverage goal the plan was derived for: `transitions`, `states`, `actions`, `scenarios` or `pairs` |
| `max_steps`, `step_budget`, `pair_limit` | the soft tour target, the hard total step ceiling and the pair sample size (null unless a sampled `pairs` plan) the plan was derived with. |
| `defaults` | the `cover`, `max_steps`, `step_budget` and `pair_limit` that `zin plan` uses when none are given. |
| `observe` | only in plans from `zin plan --observe`: the fields the adapter's `observe` is taken to return, in declaration order (empty for an adapter without `observe`). The plan's `identify` tests rule out states that look alike under these fields. |
| `summary` | full-model `states`, `transitions` and `refusals`, plus the emitted `tests`, `accepted_steps` and `rejected_steps`. `eligible_pairs` and `covered_pairs` are numbers for `pairs` plans and null otherwise. |
| `fields` | the state variables. Types: `integer` (`min`, `max`), `boolean`, `enum` (`values`), `map` (`keys` as strings, `value` type), `record` (`members`, each a `name` and a `type`), `optional` (`value` type), `set` (`values`, the element type's values as strings), `list` (`capacity`, `element` type) and `variant` (`cases`, each a `name` and its `members`) |
| `actions` | actions and their parameters, in declaration order |
| `states` | the model states the plan refers to, as objects. Integers are JSON numbers, booleans JSON booleans, enum values strings, maps objects keyed by the key's text, records objects keyed by member name, an optional that is nothing `null`, a set an array of its members in the element type's order, a list an array of its items, an enumeration case with members an object whose one key is the case. State `0` is the initial state. Only states the plan uses are listed. |
| `parents` | for each state, `[parent state, call]` on a shortest path from the initial state; `null` for state 0. Used to build the shortest reproduction. |
| `calls` | every call the plan uses: action name, positional `args` (same order as the action's `params`), and display `text` |
| `tests` | each test starts from a fresh implementation in the initial state. A step is `[call, to]`: perform `calls[call]`; if `to >= 0` it must be accepted and the model moves to `states[to]`; if `to` is `-1` it must be rejected and the model state is unchanged. `kind` is `scenario`, `witness`, `tour` or `identify` and does not change how a test is run. |

All integers fit in a JavaScript safe integer; `zin plan` refuses a
specification whose values do not.

Compound values use JSON's own structure, and a runner compares all of them:

| JSON | Meaning |
| --- | --- |
| object | a map (keys as text) or a record (keys are member names) |
| `null` | an optional value that is nothing |
| array | a list, in order, or a set, in the order its element type declares its values |

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
     value. For the sibling,
     every coverage option left out (`cover`, `max_steps`, `step_budget`,
     `pair_limit`) must equal the plan's recorded `defaults` entry, so runners
     hold no defaults and a plan with weaker coverage is never chosen
     silently. An explicit plan compares only the options passed:
     naming it is the opt-in to its coverage. `component` is identity: the
     plan's must equal the requested one.
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
rejection: the unchanged state). Observed maps and records may list a subset
of the keys, but every key must exist; types compare strictly (`false` is not
`0`, nothing is not `0`). An observed list matches an array of the same
length element by element; an observed native set (a Python `set`, a
JavaScript `Set`, a Java `Set`, a Go `zin.Set`) matches an array with the
same elements in any order. Native values are normalized first: enum members
to their name, map keys to their text, records (a Python dataclass or named
tuple, a Java record, a Go struct) to an object of their members. Anything that is neither an acceptance nor a recognized rejection
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
   "scenarios": ["Third time lucky"], "notes": [], "unguarded_reads": []},
  {"file": "stuck.zin", "ok": false, "error": {
     "kind": "violation", "code": "violation", "line": 13,
     "span": {"file": "stuck.zin", "line": 13, "column": 1, "end_column": 30}, "related": null,
     "message": "...",
     "trace": {"initial": {"status": "queued"},
               "steps": [{"call": "start", "state": {"status": "running"}}],
               "loop_from": 0},
     "fixes": []}}
]}
```

`kind` is `spec` (does not parse or type-check), `model` (an action leaves a
declared range or divides by zero), `violation`, `limit` or `usage`.
`loop_from` is `null` for a finite counterexample; otherwise the steps from
that index on repeat forever. `up_to_symmetry` is `true` when `states` and
`transitions` count representatives of interchangeable values.
`attempted_calls` is `states` times `calls`, the work exploration did, and
`relabelings` is the size of the relabeling tables it built (0 without
reduction). Readers of `check/1` ignore fields they do not know.

With `--inductive`, a successful result also has an `inductive` object:

```json
"inductive": {"holds": false, "properties": 1, "well_typed_states": 16, "satisfying_states": 13,
  "counterexample": {"state": {"attempts": 3, "status": "queued"}, "call": "start", "next": null,
    "property": null, "fault": "attempts would become 4, but ...", "file": "job.zin", "line": 31}}
```

`properties` counts the `always` and `never` properties, `well_typed_states`
every state the field types allow and `satisfying_states` those where every
such property holds. `counterexample` is `null` when `holds` is `true`.
Otherwise it gives a satisfying state, a call from it, and either the state
the call leads to (`next`) and the `property` it breaks, or `next: null` or a
state and the `fault` the call or a property raised. `file` and `line` locate
the property or the fault.

`unguarded_reads` lists the reads that can find no value and that nothing
guards, which `notes` also holds as text: each has `code`
(`read.unguarded`), `read` (`left of lease`), `subject` (`lease`), `span` (the
name that starts the read) and `message`. See
[notes](language.md#notes).

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
same rules. `zin explain <code>` explains each code; see
[diagnostic codes](errors/README.md).

`fixes` lists replacements that would fix the error, each a `span` to replace
(same rules as above), the `replacement` text and a `label` such as
``did you mean `pending`?``. Several fixes for one error are alternatives:
apply one. The list is empty when no fix is known.

## Trace events

Read by `zin trace`; see [conformance testing](conformance.md#checking-recorded-executions).
