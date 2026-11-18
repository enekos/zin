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
Fairness does not help when the call is only enabled on and off
(`examples/broken/stuck-job.zin`).

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

## Limits

Exploration stops with an error, never a partial answer, at the declared state
limit or at 20 million transitions. A specification may have at most 100,000
distinct calls (action × arguments) and 4,096 scalar slots. Nesting depth is
limited to 64.

## Notes

After a successful check the tool also notes actions that are never enabled in
any reachable state, since these are usually mistakes.
