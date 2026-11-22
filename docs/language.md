# The Zin language

A specification is a UTF-8 text file. Structure comes from indentation: a
heading line at the left margin, its body indented beneath it. `#` starts a
comment. Blank lines are ignored.

```zin
system "Name"                      # required, first
explore at most 100000 states      # optional; default 1,000,000

types                              # optional named types
  worker is one of alice, bob

state                              # the variables
  ...
initially                          # one value for every variable
  ...

always / never / sometimes / eventually     # properties
whenever <p> eventually <q>
always possible <condition>
terminal when <condition>

action name(parameters)            # how the state can change
  ...
scenario "Name"                    # worked examples, checked and turned into tests
  ...
```

## Types

| Type | Written as |
| --- | --- |
| Bounded integer | `an integer from 0 to 20` |
| Boolean | `a boolean` |
| Enumeration | `one of open, closed` or `one of open, closed or frozen` |
| Named type | `a worker` / `worker` (declared under `types`) |
| Map | `a map from worker to a boolean` |

Map keys must be finite (an enum, a boolean, or a bounded integer); map values
are any non-map type. Every key always has a value.

Enum values are plain words. Quote one (`"on"`) when it would otherwise read as
a keyword or as another name. An enum value may belong to several enums; where
that makes an expression ambiguous the checker says so.

### Interchangeable values

```zin
types
  interchangeable worker is one of ann, ben, cid, dot

state
  at is a map from worker to phase
  holder is one of ann, ben, cid, dot, nobody
```

Declare an enumeration `interchangeable` when nothing in the specification
treats one of its values differently from another: four identical workers,
three replicas, a pool of connections. `zin check` then explores one state
for each arrangement up to renaming the values (`examples/mutex.zin`: 9
states instead of 48), and a counterexample is still reported as an ordinary
run with the real names.

The checker holds you to it:

- Actions, properties and initial values may not name an interchangeable
  value. Reach them through parameters (`enter(w is a worker)`), quantifiers
  and `of every`. Scenarios may name them, since they describe one run.
- Another enumeration holds all of a type's interchangeable values or none:
  `one of ann, ben, cid, dot, nobody` is fine, `one of ann, nobody` is not.
- At most 8 interchangeable values in total (8! relabelings).

Conformance plans, `zin trace` and diagrams always use the full state space,
because tests drive real values. Liveness under fairness is also checked on
the full state space: fairness is per call, and renaming changes which call
is which. So is a model where a `some` or `every` over interchangeable values
can fault (divide by zero, overflow, read a missing map key) for one value and
stop early on another: the order the quantifier visits values in then decides
whether the fault happens, and a representative could hide it. Zin judges this
from the declared ranges, so `load of w + 1` over a bounded map keeps the
reduction. `zin check` notes why it explored the full state space. `zin check
--symmetry off` turns the reduction off.

## State and initial values

```zin
state
  balance is an integer from 0 to 20
  at is a map from worker to phase

initially
  balance is 0
  at of every worker is idle
  at of alice is holding         # single entries refine 'of every'
```

Every variable, and every key of every map, must receive an initial value.

## Expressions

From loosest to tightest binding:

| | |
| --- | --- |
| `a implies b` | right-associative |
| `a or b` | |
| `a and b` | |
| `not a` | |
| comparisons | `is`, `=`, `==`, `equals`, `is not`, `!=`, `is different from`, `<`, `<=`, `>`, `>=`, `is less than`, `is at most`, `is greater than` / `is more than`, `is at least` |
| `a + b`, `a - b` | also `plus`, `minus` |
| `a * b`, `a / b` | also `times`, `divided by`; division truncates toward zero |
| `-a` | |

Comparisons do not chain; parenthesize (`(a < b) is flag`).

Primary forms:

```zin
balance                       # a variable or parameter
5   true   closed             # literals
(balance + 1)
at of w        at[w]          # a map entry
if c then a else b            # both branches must have the same type
every w in worker satisfies at of w is done
some w in worker satisfies at of w is holding
number of w in worker where at of w is done
```

Quantifiers range over a named type (declare `small is an integer from 1 to 3`
under `types` to range over numbers) and extend as far to the right as
possible; parenthesize them inside larger expressions.

Expressions are statically typed. Integers, booleans and each enum are
distinct: `balance is open` is an error, reported before anything runs, even
in actions that can never be enabled.

Evaluation is left to right and short-circuits: `and`, `or` and `implies`
skip their right side once the left decides the result, `if` evaluates one
branch, `some` stops at its first witness and `every` at its first
counterexample, in the type's declared order. A division by zero, an
arithmetic overflow or a key outside a map's key type in a part that is
evaluated is a model fault; in a part that is skipped it is not. So
`load of w is 0 or 2 / load of w is 1` is safe where `2 / load of w is 1`
alone faults.

[Language cases](language-cases.md) pairs each of these rules with a passing
specification and a one-line change that breaks it; the test suite checks
both versions.

## Actions

```zin
fair action deposit(amount is an integer from 1 to 5, w is a worker)
  requires status is open
  requires balance + amount is at most 20
  balance becomes balance + amount
  seen of w becomes balance
  at of every worker becomes idle
```

- An action is **enabled** for given arguments when every `requires` holds.
  Otherwise it is **rejected** and the state does not change.
- All assignments read the state **before** the action: `a becomes b` and
  `b becomes a` in one action swap the two.
- Assigning the same variable or map entry twice in one action is an error.
  The one exception: `m of every <type>` sets a default that assignments to
  single entries refine.
- A variable not assigned keeps its value.
- An assignment that leaves a variable's declared range is a **model fault**,
  reported with the trace that reaches it. Add a `requires` to rule it out.
  The same applies to division by zero.
- Actions are deterministic. Model a choice made by the environment as a
  parameter, or as two actions.

Parameters range over finite types, so the checker tries every combination of
arguments in every state.

## Properties

Properties are written one per line under a heading, or on the heading line
(`never balance is 21`).

### Safety

```zin
always
  status is closed implies balance is 0
never
  balance is more than 20
```

Checked in every reachable state. A violation comes with the shortest sequence
of calls that reaches it.

### Reachability

```zin
sometimes
  status is closed
```

At least one reachable state satisfies the condition. Use this to make sure
the model is not vacuous: properties about closed accounts are worthless if no
account can be closed.

```zin
always possible status is open
```

From **every** reachable state some sequence of calls reaches the condition:
there is no point of no return.

### Liveness

```zin
eventually
  status is succeeded or status is failed

whenever status is running eventually status is not running
```

`eventually q` says every execution reaches `q`. `whenever p eventually q`
says that from every reachable state satisfying `p`, every execution reaches
`q` (possibly at once).

An **execution** is a path from a state that keeps taking steps for as long as
any call is enabled. Two consequences:

- A state where nothing is enabled ends the execution. If `q` has not happened
  by then, the property fails ("can stop before it happens").
- The system never idles while something is enabled. If your system may
  legitimately wait forever, model the waiting as an action.

Without fairness, a loop that avoids `q` is a violation even when an exit is
always available. Declare what the environment guarantees:

```zin
fair action finish
  requires not done
  done becomes true
```

`fair` is weak fairness, per call: if a call of a fair action stays enabled
forever, it is eventually taken. A violation then needs a loop in which, for
every fair call, the call is disabled somewhere on the loop or taken on it.
Weak fairness does not help when the call is only enabled on and off
(`examples/broken/stuck-job.zin`): a job that keeps crashing back to the queue
is never continuously ready to succeed.

```zin
strongly fair action succeed
  requires status is running
  status becomes succeeded
```

`strongly fair` is strong fairness, per call: if a call is enabled again and
again, forever, it is eventually taken. A violation then needs a loop that
takes every strongly fair call enabled anywhere on it. Use it for retries and
for a scheduler that keeps offering the same choice; it is a stronger promise
about the environment, so make sure the real one keeps it.

A liveness counterexample is a path followed by `then forever:` and a loop.

### Deadlock

```zin
terminal when status is closed
```

Without this line, states with no enabled call are allowed. With it, every
such state must satisfy the condition; any other is reported as a deadlock.

## Scenarios

```zin
scenario "Close and reopen an empty account"
  reject reopen                     # must be refused here
  close
  deposit with amount = 3           # named arguments
  deposit(3)                        # positional
  expect balance is 6 and status is open
```

Each scenario starts from the initial state. A call must be enabled, a
`reject` must be refused, an `expect` must hold. Scenarios document intended
behavior, pin it against regressions in the specification, and become the
first tests of every derived plan.

## Composition

Several `system` blocks in one file are checked as one system
(`examples/request-reply.zin`):

```zin
system "Client"
state
  waiting is a boolean
initially
  waiting is false
action request(n is an integer from 1 to 3)
  requires not waiting
  waiting becomes true

system "Server"
state
  job is an integer from 0 to 3
initially
  job is 0
action request(n is an integer from 1 to 3)
  requires job is 0
  job becomes n

system "Request and reply"         # no state: names the whole
always waiting implies job is not 0
```

- **Components share no state.** Field names must differ across blocks, and
  a block's actions and `initially` read and write only its own fields.
- **An action declared in several blocks is one step they all take at
  once.** It is enabled when every block's `requires` holds and applies every
  block's assignments. Parameters with the same name are the same value and
  must have the same type, which is how data passes between components; the
  merged action takes the first block's parameters, then any new ones, in
  order. It is as fair as the fairest block declares it.
- **Every other action interleaves**: it changes its own block's state only.
- **Properties, `terminal when` and scenarios describe the whole** and may
  name any block's fields. Put them in the block they concern, or in a block
  without state, whose name then names the composition. Several `terminal
  when` lines must all hold.
- Types with the same name must be declared the same way in every block that
  declares them. The largest `explore at most` wins.

Plans, traces and tests treat a composition as one system under test, unless
you test one component in isolation.

### Testing one component

```sh
zin plan examples/request-reply.zin --component Client --out client.plan.json
```

Zin checks the whole composition first, then builds a plan for the named
block alone. Its adapter implements only that block's actions and observes
only its fields. The other blocks become a controllable environment that may
offer a shared action whenever the component's own `requires` allow it, so the
plan can exercise local behavior that a particular implementation of the
other components never requests: for the client, the server-only `work`
disappears and a `reply` can follow a `request` at once.

Whole-system scenarios seed the plan. Each keeps the component's calls and its
own refusals; a call only the environment takes or refuses never reaches the
adapter. Properties and `terminal when` describe the whole and are checked
there, not on the component. The plan's `component` field names the block
(`null` for a whole system). In Rust, pass `verify_component(&spec, "Client")`
to `Plan::build`.

## Source files

A file may begin with `include` directives, then continue with complete
`system` blocks. An entry file may hold only includes.

```zin
include "parts/client.zin"
include "parts/server.zin"

system "Request and reply"
always pending is at most 1
```

Inclusion is textual, and these are deliberate choices:

- **Included text comes first.** Each file's included files, in directive
  order, precede its own blocks. The combined text is checked exactly as if it
  were written in one file, so a composition behaves the same split or whole.
- **Each file is read once.** A file included from several places, under any
  spelling of the same relative path, contributes its blocks once.
- **One flat namespace.** Included files share types, fields and actions by
  name, with the composition rules above. There are no imports, qualified
  names or private declarations. Two blocks that declare the same field, or a
  type or shared parameter differently, are an error that reports both
  declarations.
- **Paths are relative** to the including file, use `/` separators and stay
  within the entry file's directory. A cycle is an error at the include that
  closes it. At most 1,024 files nested 64 levels deep, 4 MiB in total.

`zin` commands read files this way; standard input (`-`) holds one
self-contained specification. In Rust, `parse_file(path)` loads a file with its
includes and `parse(text)` parses self-contained text, where `include` is an
error. A plan records each included file's fingerprint under `sources`, and
its `fingerprint` covers every file, so editing any of them makes a saved plan
stale.

## Diagnostics

Every error names the file, line and column it refers to:

```console
$ zin check entry.zin
spec: parts/client.zin:7:21: unknown name pendng; declare it or use a quoted enum value
```

The parser records where each name, condition, assignment and declaration
starts and ends, so a diagnostic points at the offending token, not merely its
line, even when the same name appears elsewhere on that line. An error that
concerns a whole statement covers the statement. A conflict between two
declarations also gives the first one.

`zin check --format json` adds a stable `code` (such as `name.unknown`,
`type.mismatch` or `component.field_conflict`), a `span` and a `related` span;
see [wire formats](plan-format.md#check1). In Rust, `Error::code`,
`Error::source_span` and `Error::related_span` return the same information.
Once an error leaves `parse`, `parse_file`, `Model::compile` or `verify`, its
`line` is a line of the file in `source_span`. AST nodes carry `Span`s in the
parser's coordinates; `Spec::source_span` resolves one to its file.

## Limits

Exploration stops with an error, never a partial answer, at the declared state
limit or at 20 million transitions. A specification may have at most 100,000
distinct calls (action × arguments) and 4,096 scalar slots. Nesting depth is
limited to 64.

## Notes

After a successful check the tool also notes actions that are never enabled in
any reachable state, since these are usually mistakes.
