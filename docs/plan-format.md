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
| `cover` | the coverage goal the plan was derived for: `transitions`, `states`, `actions`, `scenarios` or `pairs` |
| `max_steps`, `step_budget`, `pair_limit` | the soft tour target, the hard total step ceiling and the pair sample size (null unless a sampled `pairs` plan) the plan was derived with. Absent from plans written before 0.3. |
| `summary` | full-model `states`, `transitions` and `refusals`, plus the emitted `tests`, `accepted_steps` and `rejected_steps`. `eligible_pairs` and `covered_pairs` are numbers for `pairs` plans and null otherwise. |
| `fields` | the state variables. Types: `integer` (`min`, `max`), `boolean`, `enum` (`values`), `map` (`keys` as strings, `value` type) |
| `actions` | actions and their parameters, in declaration order |
| `states` | the model states the plan refers to, as objects. Integers are JSON numbers, booleans JSON booleans, enum values strings, maps objects keyed by the key's text. State `0` is the initial state. Only states the plan uses are listed. |
| `parents` | for each state, `[parent state, call]` on a shortest path from the initial state; `null` for state 0. Used to build the shortest reproduction. |
| `calls` | every call the plan uses: action name, positional `args` (same order as the action's `params`), and display `text` |
| `tests` | each test starts from a fresh implementation in the initial state. A step is `[call, to]`: perform `calls[call]`; if `to >= 0` it must be accepted and the model moves to `states[to]`; if `to` is `-1` it must be rejected and the model state is unchanged. |

All integers fit in a JavaScript safe integer; `zin plan` refuses a
specification whose values do not.

### What a binding must do

For each test: build a fresh implementation, then for each step perform the
call, classify the result as accepted or rejected, compare with `to`, and
compare any observed fields with the current model state (after a rejection:
the unchanged state). Compare types strictly (`false` is not `0`). Treat
anything that is neither an acceptance nor a recognized rejection as an error.

On a failure at step *n*, walk `parents` back from the model state before step
*n* to get the shortest call sequence to it, append step *n*, and rerun on a
fresh implementation. If it fails again, report it as the shortest
reproduction; if not, say that the failure depends on state the specification
does not model.

## `check/1`

Written by `zin check --format json`.

```json
{"zin": "check/1", "results": [
  {"file": "job.zin", "ok": true, "system": "Background job", "states": 10, "transitions": 9,
   "up_to_symmetry": false, "terminal_states": 4, "properties": [{"property": "eventually ...", "line": 22,
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
`transitions` count representatives of interchangeable values (an added
field; readers of `check/1` should ignore fields they do not know).

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
