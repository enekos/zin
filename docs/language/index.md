---
title: Language
nav_order: 3
has_children: true
---

# The Zin language
{: .no_toc }

A Zin specification describes a system as a finite set of **states** and the
**calls** that move between them. The checker visits every reachable state,
proves your properties over all of them, and hands the result to the test
runners as a plan.

This section is the long-form guide: why each construct exists, a complete
example of each, the real `zin` output, and the mistakes you will make. The
[one-page reference](../language.md) has the same rules in compact form.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## A first look

```zin
system "Bank account"

state
  balance is an integer from 0 to 20
  status is one of open, closed

initially
  balance is 0
  status is open

always
  status is closed implies balance is 0

action deposit(amount is an integer from 1 to 5)
  requires status is open
  requires balance + amount is at most 20
  balance becomes balance + amount

action close
  requires status is open
  requires balance is 0
  status becomes closed
```

Read it aloud and it says what it means. That is the design goal: a
specification should be reviewable by someone who has never seen Zin.

## File anatomy

A specification is a UTF-8 text file, by convention ending in `.zin`.
`system` comes first and `include` lines before it; the sections after it are
conventionally written in this order, though the checker accepts any:

```zin
include "parts/other.zin"          # optional, only at the top of a file

system "Name"                      # required: starts a block
explore at most 100000 states      # optional; default 1,000,000

types                              # optional named types
  worker is one of alice, bob

state                              # the variables
  ...
initially                          # one value for every variable
  ...

always / never / sometimes / eventually     # properties, in any order
whenever <p> eventually <q>
always possible <condition>
terminal when <condition>

action name(parameters)            # how the state can change
  ...
scenario "Name"                    # worked examples, checked and turned into tests
  ...
```

A file may hold several `system` blocks; they are then checked together as
one composed system. See [composition](composition.md).

## Layout rules

- **Indentation is structure.** A heading (`state`, `initially`, `always`,
  `action deposit(...)`, `scenario "..."`) starts at the left margin; its
  body is indented beneath it by any number of spaces. Tabs are an error
  (`use spaces, not tabs, for indentation`).
- **One item per line.** One field per line under `state`, one property per
  line under `always`, one `requires` or assignment per line in an action.
- **A property may sit on its heading line.** `never balance is 21` is the
  same as `never` followed by an indented `balance is 21`.
- **`#` starts a comment**, to the end of the line. Blank lines are ignored.
- **Names** are words of letters, digits and underscores (`give_back`,
  `s1`), and case-sensitive. Keep fields, parameters, types and enum values
  distinct from each other: where a word could mean two things, the type
  checker has to guess, and the error it gives is about the guess.

### Quoting enum values

Enum values are plain words. Declaring a value that is also a keyword
(`true`, `nothing`, `and`, `every`) is allowed, but using it bare in an
expression reads as the keyword:

```console
$ zin check q.zin
spec: q.zin:8:13: expected a value or name, found 'and'
```

Quote it wherever it would read as something else: `"on"`, `"nothing"`,
`"and"`. Quoting is always allowed, so when in doubt, quote.

```zin
system "Switch"
state
  power is one of "nothing", off
initially
  power is "nothing"
action go
  requires power is "nothing"
  power becomes off
```

## The mental model

Everything else in this guide follows from five ideas.

1. **A state** is one value for every field. The checker starts from the one
   state described by `initially`.
2. **A call** is an action with concrete arguments: `deposit(3)`. Parameters
   range over finite types, so a specification has a finite number of calls,
   and the checker tries every call in every state it reaches.
3. **Enabled or rejected.** A call is *enabled* in a state when every
   `requires` line holds. It then moves the system to a new state. Otherwise
   it is *rejected*: the state does not change. A rejection is a normal,
   specified outcome. It is also what the conformance tests check your
   implementation refuses.
4. **Reachable states.** Starting from the initial state and taking enabled
   calls in every possible order gives the reachable states. Properties are
   proven over all of them. Exploration is exhaustive and breadth-first, so
   every safety counterexample is a shortest one.
5. **Model faults.** A call that would put a field outside its declared range,
   divide by zero, or read a value that is not there (an empty optional, an
   empty list) is a *fault* in the specification, not a rejection. `zin
   check` reports it with the trace that reaches it. Fix it with a
   `requires` that rules the step out.

```console
$ zin check tank.zin
model: tank.zin:7:3: add: balance would become 4, but it is an integer from 0 to 3; add a 'requires' that rules this out
counterexample:
  start: balance=0
  1. add  ->  balance=1
  2. add  ->  balance=2
  3. add  ->  balance=3
```

The same model is the oracle for the tests: for every reachable state and
every call, the specification says *accept and move here* or *reject*. The
[bindings](../bindings/index.md) replay those answers against your code.

## What `zin check` tells you

| Output starts with | Meaning | Exit code |
| --- | --- | --- |
| `ok` | every property and scenario holds in every reachable state | 0 |
| `violation:` | a property or scenario fails; a counterexample follows | 1 |
| `model:` | a reachable step faults (range, division, absent value) | 1 |
| `spec:` | the text does not parse or does not type-check | 2 |
| `limit:` | exploration hit a limit before it could decide | 2 |
| `usage:` | the command line or a file it names cannot be used | 2 |

`ok` lines may be followed by `note:` lines: actions that are never enabled,
and reads that nothing guards. See [diagnostics](diagnostics.md).

## Cheat sheet

| Construct | Example | Page |
| --- | --- | --- |
| Bounded integer | `an integer from 0 to 20` | [Types](types.md#integers) |
| Boolean | `a boolean` | [Types](types.md#booleans) |
| Enumeration | `one of open, closed` | [Types](types.md#enumerations) |
| Named type | `types` / `worker is one of ann, ben` | [Types](types.md#named-types) |
| Interchangeable values | `interchangeable worker is one of ann, ben` | [Types](types.md#interchangeable-values) |
| Map | `a map from worker to a boolean` | [Types](types.md#maps) |
| Record | `a record with (status is ..., items is ...)` | [Types](types.md#records) |
| Optional | `a worker or nothing` | [Types](types.md#optional-values) |
| Set | `a set of worker` | [Types](types.md#sets) |
| List | `a list of at most 3 message` | [Types](types.md#lists) |
| Enumeration with members | `one of idle, running with (attempt is ...)` | [Types](types.md#enumerations-with-members) |
| Initial values | `at of every worker is idle` | [State and actions](state-and-actions.md#initial-values) |
| Action | `action deposit(amount is an integer from 1 to 5)` | [State and actions](state-and-actions.md#actions) |
| Guard | `requires balance + amount is at most 20` | [State and actions](state-and-actions.md#requires) |
| Assignment | `balance becomes balance + amount` | [State and actions](state-and-actions.md#assignments) |
| Fairness | `fair action`, `strongly fair action` | [Properties](properties.md#fairness) |
| Arithmetic, comparison, logic | `a + b`, `is at most`, `implies` | [Expressions](expressions.md) |
| Quantifiers | `every w in worker satisfies ...`, `number of w in worker where ...` | [Expressions](expressions.md#quantifiers) |
| Conditional | `if c then a else b` | [Expressions](expressions.md#if-then-else) |
| Default | `left of lease or else 0` | [Expressions](expressions.md#defaults-with-or-else) |
| Safety | `always ...`, `never ...` | [Properties](properties.md#safety-always-and-never) |
| Reachability | `sometimes ...`, `always possible ...` | [Properties](properties.md#reachability) |
| Liveness | `eventually ...`, `whenever p eventually q` | [Properties](properties.md#liveness) |
| Deadlock | `terminal when ...` | [Properties](properties.md#deadlock-terminal-when) |
| Scenario | `scenario "..."` / `reject close` / `expect ...` | [Scenarios](scenarios.md) |
| Composition | several `system` blocks, shared actions | [Composition](composition.md) |
| Includes | `include "parts/client.zin"` | [Composition](composition.md#includes) |
| State limit | `explore at most 100000 states` | [Diagnostics](diagnostics.md#limits) |

## Where to go next

- New to Zin: [Getting started](../getting-started.md) walks through a whole
  specification, its proof and its tests.
- Writing a model: [Types](types.md), then [State and actions](state-and-actions.md),
  then [Properties](properties.md).
- Stuck on an idea: [Modeling patterns](patterns.md).
- Stuck on an error: [Diagnostics and fixes](diagnostics.md).
