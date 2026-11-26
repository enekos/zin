---
title: Diagnostics and fixes
parent: Language
nav_order: 8
---

# Diagnostics and fixes
{: .no_toc }

Every problem `zin check` reports has a kind, a location, a stable code and,
where it can, a suggested fix that `--fix` applies for you.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Anatomy of an error

```console
$ zin check typo.zin
spec: typo.zin:7:22: unknown name drat; declare it or use a quoted enum value
  hint: did you mean `draft`?
    7 |   requires status is draft
      |                      ^^^^^
  = see: zin explain name.unknown
```

| Part | Meaning |
| --- | --- |
| `spec:` | the kind: `spec`, `model`, `violation`, `limit` or `usage` |
| `typo.zin:7:22` | file, line and column of the offending token |
| the message | what is wrong, in terms of your specification |
| `hint:` | a suggested replacement, shown as the line would read with it |
| `^^^^^` | the span the fix would replace |
| `= see:` | the code to look up with `zin explain` |

The parser records where every name, condition, assignment and declaration
starts and ends, so the location is the token, not just the line, even when
the same name appears elsewhere on it. An error about a whole statement
covers the statement. A conflict between two declarations also names the
first one (`first declared at c2.zin:3:3`). With [includes](composition.md#includes)
the location is the included file that holds the text.

## Kinds and exit codes

| Kind | Meaning | Exit |
| --- | --- | --- |
| `spec` | does not parse or type-check; nothing was explored | 2 |
| `usage` | the command line or a file it names cannot be used | 2 |
| `limit` | exploration stopped at a limit before it could decide | 2 |
| `model` | a reachable step leaves a range, divides by zero or reads an absent value | 1 |
| `violation` | a property or scenario fails; a counterexample follows | 1 |

Exit code 0 means every file checked passed. `zin check a.zin b.zin` checks
several files and fails if any does.

## Diagnostic codes

The text output ends with `= see: zin explain <code>` when a finer code than
the kind applies. `zin explain` lists them; `zin explain <code>` prints a
specification that gets the error, the exact output, and the fix:

```console
$ zin explain
spec                          The specification cannot be read: a syntax error or a rule of the language it breaks.
usage                         The command line or a file it names cannot be used.
model.fault                   Some reachable step leaves a declared range, divides by zero or reads a value that is not there.
violation                     A property does not hold; the counterexample shows how.
limit                         Exploration stopped at a limit before it could decide.
name.unknown                  A name that nothing declares.
type.mismatch                 A value of one type where another is needed.
component.field_conflict      Two system blocks declare the same state field.
component.duplicate_action    One system block declares an action twice.
component.foreign_field       A system block touches a field that another block owns.
component.type_conflict       Two system blocks declare a type under one name with different values.
component.parameter_conflict  Two system blocks give a shared action's parameter different types.
```

Each code has a page: [diagnostic codes](../errors/README.md). New codes may
be added; a code is never reused for a different meaning.

## Suggested fixes

When a name is unknown and a declared name of a fitting kind is close to it,
the error suggests that name.

- **Close** means one edit for names of up to four characters and two for
  longer ones (insert, delete, substitute, or swap neighbors), or a
  difference only in case.
- **Fitting** means the right type: in `status is pendng`, where `status` is
  a phase, only phases are suggested.
- **Ties are all offered**, as alternatives:

  ```console
  $ zin check mode.zin
  spec: mode.zin:7:20: unknown name cap; declare it or use a quoted enum value
    hint: did you mean `car`?
      7 |   requires mode is car
        |                    ^^^
    hint: did you mean `cat`?
      7 |   requires mode is cat
        |                    ^^^
    = see: zin explain name.unknown
  ```

- **A missing include** suggests the closest `.zin` file next to it.

### `zin check --fix`

`--fix` applies each suggestion that is the only one for its error, checks
again, and repeats until the specification checks or an error has no single
fix. It edits the file in place and prints each change:

```console
$ zin check --fix typo.zin
fixed typo.zin:7:22: drat -> draft
fixed typo.zin:8:18: pendng -> pending
ok Typo: 3 states, 2 transitions, 0 properties, 0 scenarios
```

Alternatives are left for you to choose: `--fix` on `mode.zin` above changes
nothing and exits 2.

## Machine-readable output

`zin check --format json` prints one `check/1` document for all files. A
failure carries the code, the spans, the counterexample and any fixes:

```console
$ zin check --format json typo.zin
```

```json
{
  "zin": "check/1",
  "results": [
    {
      "file": "typo.zin",
      "ok": false,
      "error": {
        "kind": "spec",
        "code": "name.unknown",
        "line": 7,
        "span": {"file": "typo.zin", "line": 7, "column": 22, "end_column": 26},
        "related": null,
        "message": "unknown name drat; declare it or use a quoted enum value",
        "trace": null,
        "fixes": [
          {
            "span": {"file": "typo.zin", "line": 7, "column": 22, "end_column": 26},
            "replacement": "draft",
            "label": "did you mean `draft`?"
          }
        ]
      }
    }
  ]
}
```

A violation's `trace` has the initial state, each step's call and full
state, and `loop_from` (the index the loop starts at for a liveness
counterexample, `null` otherwise):

```json
"trace": {
  "initial": {"alarm": false, "door": "closed"},
  "steps": [
    {"call": "lock", "state": {"alarm": false, "door": "locked"}},
    {"call": "force", "state": {"alarm": true, "door": "locked"}}
  ],
  "loop_from": null
}
```

A success lists the counts, each property with its span, scenarios, `notes`
and `unguarded_reads`. Columns count Unicode scalar values from one and
`end_column` is exclusive. The [wire formats](../plan-format.md#check1) page
has every field. Editors and CI annotations can use the spans directly.

From Rust, `Error::code`, `Error::source_span`, `Error::related_span`,
`Error::fixes` and `Error::report` return the same information.

## Notes

After a successful check, `zin check` adds notes for things that are legal
but usually wrong. They do not change the exit code.

### Actions never enabled

```console
$ zin check never.zin
ok Never: 3 states, 2 transitions, 0 properties, 0 scenarios
  note: action reset is never enabled in any reachable state
```

An action no reachable state enables is dead code in the model, and the plan
can only test that it is always refused. Usually a `requires` is too strict
or a range too small.

### Unguarded reads

A read that can find no value (an optional that may be nothing, a case
member, an item of a possibly empty list) is noted when nothing guards it,
even if no reachable state makes it fault:

```zin
system "Lease"
types
  worker is one of ann, ben
state
  lease is a record with (holder is a worker, left is an integer from 0 to 2) or nothing
  ticks is an integer from 0 to 2
initially
  lease is nothing
  ticks is 0
action acquire(w is a worker)
  requires lease is nothing
  lease becomes {holder: w, left: 2}
  ticks becomes 1
action tick
  requires ticks is 1
  ticks becomes left of lease
  lease becomes nothing
```

```console
$ zin check lease.zin
ok Lease: 4 states, 6 transitions, 0 properties, 0 scenarios
  note: lease.zin:16:17: 'left of lease' faults if lease is nothing; no reachable state gets there, but nothing guards it: write 'lease is not nothing and ...' before it, or 'left of lease or else ...'
```

`tick` is safe only because `ticks` happens to be 1 exactly while there is a
lease. A change elsewhere can break that. Write the guard (`requires lease is
not nothing`) or the default (`left of lease or else 0`). The rules for what
counts as a guard are in [expressions](expressions.md#guards-the-checker-recognizes).

In JSON each one is also listed under `unguarded_reads`:

```json
{"code": "read.unguarded", "read": "left of lease", "subject": "lease",
 "span": {"file": "lease.zin", "line": 16, "column": 17, "end_column": 21},
 "message": "'left of lease' faults if lease is nothing; ..."}
```

### Symmetry and induction

With [interchangeable values](types.md#interchangeable-values), `zin check`
notes when it had to explore the full state space, and why:

```console
$ zin check pool.zin
ok S: 9 states, 12 transitions, 2 properties, 0 scenarios
  note: symmetry reduction is off: liveness under fairness is checked on the full state space
```

With `--inductive`, a specification without `always` or `never` properties
gets a note instead of a verdict:

```console
$ zin check --inductive flag.zin
ok S: 1 state, 0 transitions, 0 properties, 0 scenarios
  note: there are no always or never properties to check for induction
```

## Limits

Exploration stops with an error, never a partial answer:

| Limit | Default | Change it |
| --- | --- | --- |
| reachable states | 1,000,000 | `explore at most N states` after `system` |
| transitions | 20 million | (fixed) |
| distinct calls (action × arguments) | 100,000 | (fixed) |
| scalar slots | 4,096 | (fixed) |
| nesting depth | 64 | (fixed) |
| interchangeable values | 8 | (fixed) |

```zin
system "Big"
explore at most 50 states
state
  a is an integer from 0 to 9
  b is an integer from 0 to 9
initially
  a is 0
  b is 0
action ia
  requires a < 9
  a becomes a + 1
action ib
  requires b < 9
  b becomes b + 1
```

```console
$ zin check big.zin
big.zin: limit: more than 50 states are reachable; tighten the ranges or raise the limit with 'explore at most N states'
```

Hitting a limit means the model is larger than you think. Before raising it,
check whether a range can shrink or a field can go; see
[keep ranges small](patterns.md#keep-ranges-small).

## Common errors and their fixes

| Message | Usual fix |
| --- | --- |
| `unknown name X; declare it or use a quoted enum value` | typo (try `--fix`), or quote an enum value that reads as a keyword |
| `cannot compare a number with an enum value` | compare like with like; check which field you meant |
| `X has no initial value; set it in the initially block` | add it, or `of every` for maps |
| `this value is already assigned in the same block` | one write per slot per action; merge into one `if` |
| `initial values must be constants` | use a literal; derive the rest with an action |
| `only a state field, or an entry or member of one, can be assigned` | parameters are read-only; optional records and cases are assigned whole |
| `... would become N, but it is an integer from A to B` | add a `requires` that refuses the step |
| `division by zero while evaluating ...` | guard with `x is 0 or ...` on the left |
| `X is nothing` / `job is not running, so it has no attempt` | guard the read or use `or else` |
| `queue is full` | require `length of queue < N` before appending |
| `running carries members; write 'running with {...}'` | give the case its members |
| `both branches of 'if' must have the same kind of value` | make both branches one type |
| `expected 'satisfies', found 'integer'` | quantify over a named type, not an inline range |
| `unexpected '<='` | comparisons do not chain; use `and` |
| `use spaces, not tabs, for indentation` | replace tabs |
