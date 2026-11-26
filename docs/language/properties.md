---
title: Properties
parent: Language
nav_order: 4
---

# Properties
{: .no_toc }

Properties are what `zin check` proves. Each form has a fixed, plain-English
reading and a fixed kind of counterexample.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## The forms

| You write | Zin proves, over every reachable state | A counterexample is |
| --- | --- | --- |
| `always c` | `c` holds in all of them | the shortest run to a state where it fails |
| `never c` | `c` holds in none of them | the shortest run to a state where it holds |
| `sometimes c` | at least one satisfies `c` | (none: no state does) |
| `always possible c` | from every one, some run still reaches `c` | a run to a point of no return |
| `eventually c` | every execution reaches `c` | a run that stops, or loops forever, without `c` |
| `whenever p eventually q` | after any `p` state, every execution reaches `q` | the same, starting at a `p` state |
| `terminal when c` | every state with nothing enabled satisfies `c` | the shortest run to a deadlock |

Write properties one per line under a heading, or on the heading line:

```zin
always
  status is closed implies balance is 0
  balance is at most 20
never balance is 21
```

Properties may name any field, use any [expression](expressions.md), and in a
[composition](composition.md) may span several blocks.

## A worked example

```zin
system "Door"

state
  door is one of closed, open, locked
  alarm is a boolean

initially
  door is closed
  alarm is false

always
  door is open implies not alarm
never
  alarm and door is not locked
sometimes
  alarm
always possible door is closed

action open
  requires door is closed
  door becomes open

action close
  requires door is open
  door becomes closed

action lock
  requires door is closed
  door becomes locked

action unlock
  requires door is locked
  requires not alarm
  door becomes closed

action force
  requires door is locked
  requires not alarm
  alarm becomes true

action reset
  requires alarm
  alarm becomes false
```

```console
$ zin check door.zin
ok Door: 4 states, 6 transitions, 4 properties, 0 scenarios
```

The first draft of this door had no `requires not alarm` on `unlock`. The
checker found the way to a ringing alarm on an unlocked door:

```console
$ zin check door.zin
violation: door.zin:14:3: 'never alarm and door is not locked' is violated
counterexample:
  start: alarm=false, door=closed
  1. lock  ->  door=locked
  2. force  ->  alarm=true
  3. unlock  ->  door=closed
```

The sections below take each form in turn.

## Safety: `always` and `never`

```zin
always
  status is closed implies balance is 0
never
  balance is more than 20
```

Checked in every state as it is discovered. Exploration is breadth-first, so
the counterexample is a **shortest** sequence of calls from the initial
state, with the state after each step (only changed fields are printed).

`never c` is `always not c`; use whichever reads better. `implies` is the
usual connective: `always p implies q` says nothing about states where `p` is
false.

Safety violations are the most common and most useful output of `zin check`.
Each one is also a ready-made regression test: paste the steps into a
[scenario](scenarios.md).

### Inductive invariants

`zin check --inductive` asks a stronger question: do the `always` and `never`
properties hold *because of the actions*, or only because of facts about
which states are reachable that the specification never states?

It tries every well-typed state where all of them hold, reachable or not, and
checks that every call from it leads to such a state again.

```zin
system "Lockstep"
state
  a is an integer from 0 to 3
  b is an integer from 0 to 3
initially
  a is 0
  b is 0
always
  b is at most 2
action step
  requires a < 2
  a becomes a + 1
  b becomes b + 1
```

```console
$ zin check --inductive lockstep.zin
ok Lockstep: 3 states, 2 transitions, 1 property, 0 scenarios
  not inductive: step breaks 'always b is at most 2' (lockstep.zin:9) from a state where the always/never property holds
    state: a=0, b=2
    step  ->  a=1, b=3
  no reachable state looks like this one, but the properties do not rule it out; add an 'always' that does, so that each call preserves them on its own
```

`b is at most 2` holds only because `b` always equals `a` and `a` stops at 2.
State it, and the properties become inductive:

```zin
always
  b is at most 2
  a is b
```

```console
$ zin check --inductive lockstep.zin
ok Lockstep: 3 states, 2 transitions, 2 properties, 0 scenarios
  inductive: the 2 always/never properties still hold after every call from any of the 3 of 16 well-typed states where they hold
```

Why bother, when the plain check already proved the property? An inductive
set of properties explains *why* the system is safe one action at a time. It
is what you need to reason about one method of the implementation in
isolation, and it keeps holding when someone adds a new way to reach a state.

- A specification that is not inductive still passes (exit code 0).
- The counterexample is the first found, trying fields in declaration order
  and values in declared order.
- Every well-typed state is enumerated, so their number must fit in the
  state limit.
- Without `always` or `never` properties you get a note instead.

## Reachability

### `sometimes`

```zin
sometimes
  status is closed
  renewals is 2
```

At least one reachable state satisfies the condition. Use it against
**vacuity**: a property about closed accounts proves nothing if no account
can ever be closed. A `requires` that is accidentally impossible is caught
here:

```console
$ zin check vac.zin
violation: vac.zin:9:1: 'sometimes status is closed' fails: no reachable state satisfies it
```

Each `sometimes` also becomes a test in the derived plan (named `reaches:
...`), which drives the implementation to that state.

Write a `sometimes` for every interesting state you expect: the full queue,
the last retry, the failure case. They cost little and catch over-constrained
models early.

### `always possible`

```zin
always possible door is closed
```

From **every** reachable state, some sequence of calls still reaches the
condition: there is no point of no return. Remove the door's `reset` action
and the alarm becomes one:

```console
$ zin check door.zin
violation: door.zin:17:1: 'always possible door is closed' fails: after this execution it can never happen again
counterexample:
  start: alarm=false, door=closed
  1. lock  ->  door=locked
  2. force  ->  alarm=true
```

`always possible` says recovery *can* happen. To say it *will* happen, use
`eventually` with fairness.

## Liveness

```zin
eventually
  status is succeeded or status is failed

whenever status is running eventually status is not running
```

`eventually q`: every execution from the initial state reaches `q`.
`whenever p eventually q`: from every reachable state satisfying `p`, every
execution reaches `q` (possibly at once).

An **execution** keeps taking steps for as long as any call is enabled. Two
consequences:

- **Stopping counts.** A state where nothing is enabled ends the execution;
  if `q` has not happened, the property fails:

  ```console
  $ zin check stop.zin
  violation: stop.zin:6:1: 'eventually s is c' fails: the system can stop before it happens
  counterexample:
    start: s=a
    1. go  ->  s=b
  ```

- **The system never idles while something is enabled.** If your real system
  may legitimately wait forever, model the waiting as an action.

A liveness counterexample that does not stop is a path followed by `then
forever:` and a loop.

### Fairness

Without any fairness, any loop that avoids `q` is a violation, even when a
way out is always available:

```zin
system "Spin"
state
  done is a boolean
  n is an integer from 0 to 1
initially
  done is false
  n is 0
eventually done
action spin
  requires not done
  n becomes 1 - n
action finish
  requires not done
  done becomes true
```

```console
$ zin check spin.zin
violation: spin.zin:8:1: 'eventually done' fails: the system can run forever without it happening
counterexample:
  start: done=false, n=0
  then forever:
  1. spin  ->  n=1
  2. spin  ->  n=0
```

Fairness is a promise about the environment, declared per action:

```zin
fair action finish
  requires not done
  done becomes true
```

```console
$ zin check spin.zin
ok Spin: 4 states, 4 transitions, 1 property, 0 scenarios
```

**`fair` (weak fairness), per call**: if a call of the action stays enabled
forever, it is eventually taken. A violating loop must, for every fair call,
either disable it somewhere on the loop or take it.

**`strongly fair` (strong fairness), per call**: if a call is enabled again
and again, forever, it is eventually taken, even if it is also disabled again
and again. A violating loop must take every strongly fair call that is enabled
anywhere on it.

The difference shows up with retries. This printer jams and goes back to the
queue:

```zin
system "Printer"

state
  job is one of idle, queued, printing, done
  paper is a boolean

initially
  job is idle
  paper is true

whenever job is queued eventually job is done

action submit
  requires job is idle
  job becomes queued

fair action start
  requires job is queued and paper
  job becomes printing

fair action finish
  requires job is printing
  job becomes done

action jam
  requires job is printing
  job becomes queued
  paper becomes false

fair action refill
  requires not paper
  paper becomes true
```

```console
$ zin check printer.zin
violation: printer.zin:11:1: 'whenever job is queued eventually job is done' fails: the system can run forever without it happening
counterexample:
  start: job=idle, paper=true
  1. submit  ->  job=queued
  then forever:
  2. start  ->  job=printing
  3. jam  ->  job=queued, paper=false
  4. refill  ->  paper=true
```

`finish` is weakly fair, but it is never *continuously* enabled: every jam
disables it. Declaring `strongly fair action finish` (a printer that keeps
getting to print eventually finishes) makes the property hold:

```console
$ zin check printer.zin
ok Printer: 5 states, 5 transitions, 1 property, 0 scenarios
```

{: .warning }
Fairness is an assumption, not a fact Zin checks about your code. Declare it
only where the real environment keeps the promise: a scheduler that does not
starve a worker, a network that eventually delivers. A strongly fair action
is a bigger promise than a fair one.

Notes:

- Fairness is **per call**: `fair action send(i is an item)` promises that
  each `send(a)` and each `send(b)` is eventually taken, not merely some
  `send`.
- In a [composition](composition.md), a shared action is as fair as the
  fairest block declares it.
- Liveness under fairness is always checked on the full state space, without
  [symmetry reduction](types.md#interchangeable-values).
- Liveness is proven about the model only. A test cannot show that an
  implementation eventually does something.

## Deadlock: `terminal when`

```zin
terminal when status is succeeded or status is failed
```

Without this line, a state where no call is enabled is allowed. With it,
every such state must satisfy the condition; any other is reported as a
deadlock with the shortest run to it. Here the door without `reset`, and
`terminal when door is open`:

```console
$ zin check door.zin
violation: door.zin:40:1: deadlock: no action is enabled, but 'terminal when door is open' does not hold
counterexample:
  start: alarm=false, door=closed
  1. lock  ->  door=locked
  2. force  ->  alarm=true
```

Use it whenever a system is meant to run forever (`terminal when false`
forbids stopping entirely) or to stop only in final states. In a composition,
several `terminal when` lines must all hold.

## Choosing properties

A useful order when writing a new specification:

1. `sometimes` for each state you expect to reach: catches a model that does
   nothing.
2. `always` / `never` for each rule that must never break.
3. `terminal when` if the system must not get stuck.
4. `always possible` for recovery: no point of no return.
5. `eventually` / `whenever ... eventually` with the fairness the environment
   really gives.
6. `zin audit` to find the rules no property pins down. See
   [auditing](../conformance.md#auditing-the-specification).
