---
title: Modeling patterns
parent: Language
nav_order: 7
---

# Modeling patterns
{: .no_toc }

Recipes for the questions every new specification raises: where does the
server go, how do I say "retry", how do I model time. Every example passes
`zin check`.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## The environment is actions too

A system is rarely wrong about one call in isolation. It is wrong about
order: the answer that arrives after the user moved on, the second click
before the first one finished. So put the environment's moves into the
specification as actions next to the user's.

```zin
system "Autosave"

state
  draft is an integer from 0 to 2
  saved is an integer from 0 to 2
  sent is an integer from 0 to 2
  status is one of idle, saving, failed

initially
  draft is 0
  saved is 0
  sent is 0
  status is idle

always
  status is saving or sent is saved

sometimes
  status is failed
  saved is 2
  status is saving and draft is not sent

whenever status is saving eventually status is not saving

action edit
  requires draft < 2
  draft becomes draft + 1

action save
  requires status is not saving
  requires draft is not saved
  sent becomes draft
  status becomes saving

fair action server_ok
  requires status is saving
  saved becomes sent
  status becomes idle

fair action server_fails
  requires status is saving
  sent becomes saved
  status becomes failed
```

```console
$ zin check autosave.zin
ok Autosave: 13 states, 19 transitions, 5 properties, 0 scenarios
```

- `save` records **what was sent** (`sent`), because the server stores that,
  not whatever the draft is when the answer arrives. The `sometimes status is
  saving and draft is not sent` line proves the race (an edit while saving)
  is reachable, so it is explored.
- `server_ok` and `server_fails` are the server's answers. The checker
  explores every interleaving with `edit` and `save`.
- In the tests, the adapter *holds* the request and answers it when the plan
  says `server_ok`, so a race replays deterministically. See
  [checking a web interface](../browser.md) for `gate`, which does this in
  Playwright.

The full version is
[`examples/editor.zin`](https://github.com/enekos/zin/blob/main/examples/editor.zin).

## Nondeterminism through parameters

Actions are deterministic. A choice the world makes becomes a parameter, and
the checker tries every value:

```zin
system "Payment"

state
  status is one of new, pending, paid, declined
  tries is an integer from 0 to 2

initially
  status is new
  tries is 0

sometimes
  status is paid
  status is declined

eventually
  status is paid or status is declined

fair action charge
  requires status is new and tries < 2
  status becomes pending
  tries becomes tries + 1

fair action answer(ok is a boolean, retryable is a boolean)
  requires status is pending
  status becomes if ok then paid else if retryable and tries < 2 then new else declined

terminal when status is paid or status is declined
```

```console
$ zin check payment.zin
ok Payment: 8 states, 10 transitions, 3 properties, 0 scenarios
```

`answer(true, false)`, `answer(false, true)` and so on are separate calls.
The alternative, one action per outcome (`answer_ok`, `answer_retryable`,
`answer_declined`), reads better when outcomes change different fields.
Either way your test adapter drives the chosen outcome into a stub.

## Retries and strong fairness

A bounded retry needs no fairness: the attempt counter makes every loop
finite. An unbounded one does, and the kind matters:

```zin
system "Retry forever"

state
  status is one of waiting, sending, delivered

initially
  status is waiting

eventually status is delivered

fair action send
  requires status is waiting
  status becomes sending

action drop
  requires status is sending
  status becomes waiting

fair action ack
  requires status is sending
  status becomes delivered
```

```console
$ zin check delivery.zin
violation: delivery.zin:9:1: 'eventually status is delivered' fails: the system can run forever without it happening
counterexample:
  start: status=waiting
  then forever:
  1. send  ->  status=sending
  2. drop  ->  status=waiting
```

`ack` is weakly fair, but every `drop` disables it, so it is never
continuously enabled. Promise that a message sent again and again is
eventually acknowledged with `strongly fair action ack`:

```console
$ zin check delivery.zin
ok Retry forever: 3 states, 3 transitions, 1 property, 0 scenarios
```

Only make that promise if the real network keeps it. See
[fairness](properties.md#fairness).

## Queues and channels

A bounded list is a FIFO channel: append with `with`, take with `first of`
and `rest of`.

```zin
action send(m is a message)
  requires length of queue < 2
  queue becomes queue with m

fair action deliver
  requires queue is not []
  received becomes received with first of queue
  queue becomes rest of queue
```

Guard every `first of` / `rest of` with `queue is not []`, and every append
with a length check; the checker reports a full list or an empty read as a
model fault otherwise. For an unordered mailbox, use a set; for a
least-recently-used order, `order without k with k` moves `k` to the end.
See [`examples/channel.zin`](https://github.com/enekos/zin/blob/main/examples/channel.zin)
and [`examples/recency.zin`](https://github.com/enekos/zin/blob/main/examples/recency.zin).

## Locks and identical workers

Several identical actors are a map from an interchangeable type, and their
actions take the actor as a parameter:

```zin
types
  interchangeable worker is one of ann, ben, cid, dot
  phase is one of idle, waiting, critical

state
  at is a map from worker to phase
  holder is one of ann, ben, cid, dot, nobody

never
  some a in worker satisfies some b in worker satisfies a is not b and at of a is critical and at of b is critical

action enter(w is a worker)
  requires at of w is waiting and holder is nobody
  at of w becomes critical
  holder becomes w
```

`interchangeable` lets the checker explore one state per arrangement up to
renaming: 9 states instead of 48 here. See
[interchangeable values](types.md#interchangeable-values) and
[`examples/mutex.zin`](https://github.com/enekos/zin/blob/main/examples/mutex.zin).

For a race between read and write, give each actor a program counter and a
local copy, and split the operation into steps. The checker finds the lost
update in
[`examples/broken/lost-update.zin`](https://github.com/enekos/zin/blob/main/examples/broken/lost-update.zin).

## Time as tick actions

Zin has no clocks. Model the passing of time as an action, and a timeout as
a counter that the tick advances:

```zin
system "Session with timeout"

types
  tick is an integer from 0 to 2

state
  session is one of signed_out, active with (idle is tick)

initially
  session is signed_out

always
  session is signed_out or idle of session <= 2
sometimes
  session is active with {idle: 2}
always possible session is signed_out

action sign_in
  requires session is signed_out
  session becomes active with {idle: 0}

action use
  requires session is active
  session becomes active with {idle: 0}

action tick
  requires session is active
  session becomes if idle of session is 2 then signed_out else active with {idle: idle of session + 1}

action sign_out
  requires session is active
  session becomes signed_out

scenario "Three idle ticks sign you out"
  sign_in
  tick
  tick
  reject sign_in
  tick
  expect session is signed_out
  reject tick
```

```console
$ zin check session.zin
ok Session with timeout: 4 states, 10 transitions, 3 properties, 1 scenario
```

The model says nothing about how long a tick is; that is a property of your
implementation's clock. In the tests, the adapter's `tick` advances a fake
clock past one interval.

## Variants instead of stale side fields

A status enum with side fields lets the fields hold values that mean nothing:

```zin
state
  status is one of idle, running
  attempt is an integer from 0 to 2      # meaningless while idle
```

Every stale combination is a distinct state, so the model is larger and the
tests compare values the implementation is free to forget. An enumeration
case with members ties the field to the case:

```zin
state
  job is one of idle, running with (attempt is an integer from 1 to 2)
```

Now `attempt of job` exists only while running, and reading it in another
case is a fault the checker points at. See
[enumerations with members](types.md#enumerations-with-members). The same
goes for "no value yet": use `or nothing` rather than a sentinel `0` or
`nobody`.

## Keep ranges small

Every value in a range is a state the checker visits and the tests replay.
A cart counter from 0 to 100 against one from 0 to 3:

```console
$ zin plan cart.zin --out cart.plan.json
ok Cart: 101 states, 200 transitions, 0 properties, 0 scenarios
plan: 2 tests, 201 accepted steps, 2 rejected steps (cover: transitions) -> cart.plan.json
$ zin plan cart.zin --out cart.plan.json
ok Cart: 4 states, 6 transitions, 0 properties, 0 scenarios
plan: 1 tests, 6 accepted steps, 2 rejected steps (cover: transitions) -> cart.plan.json
```

Both find the same defects at the boundaries (empty, one, full). Choose the
smallest range that still has a "middle": usually 0 to 2 or 0 to 3. When the
real limit matters (a quota of 20), keep the real number in the
implementation and test a configurable copy with a small limit, or model the
quota as `below`, `at` and `over`.

The plan has roughly `states × calls` steps. For a slow implementation (a
browser, a database), small ranges are the difference between seconds and
an hour.

## `sometimes` against vacuity

A specification that can do nothing satisfies every `always`. A typo in a
`requires` can make an important action impossible:

```zin
action close
  requires status is open and balance is 0 and balance > 0
  status becomes closed
```

The safety properties still pass. A `sometimes` for each state you care
about catches it:

```console
$ zin check vacuous.zin
violation: vacuous.zin:9:1: 'sometimes status is closed' fails: no reachable state satisfies it
```

`zin check` also notes actions that are never enabled. Write a `sometimes` for
the full queue, the last retry, the failure path: each one also becomes a
`reaches:` test in the plan that drives the implementation there.

## `always possible` for recovery

`always possible c` says no state is a dead end with respect to `c`: from
anywhere, some run still gets back. It is the property for "the system can
always recover", "a user can always sign out", "the lock can always be
freed":

```zin
always possible holder is nobody
always possible session is signed_out
```

It holds even without fairness, because it asks whether a way back exists,
not whether it is taken. When you need the way back to be *taken*, use
`whenever p eventually q` with fairness on the recovering actions.

## Check the specification itself

- `zin audit spec.zin` drops and alters each rule in turn and reports the
  changes no property or scenario notices. A survivor is a missing property,
  a missing scenario line, or a redundant rule.
- `zin check --inductive spec.zin` asks whether the `always` properties
  explain themselves action by action. See
  [inductive invariants](properties.md#inductive-invariants).
- `zin observe spec.zin --fields a,b` asks whether a test that observes only
  `a` and `b` can tell every state apart. See
  [what observe can tell apart](../conformance.md#what-observe-can-tell-apart).
