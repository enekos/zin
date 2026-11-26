---
title: State and actions
parent: Language
nav_order: 2
---

# State and actions
{: .no_toc }

`state` declares what the system remembers, `initially` where it starts, and
each `action` one way it can change. Together they define the state graph
that every property, plan and trace is checked against.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## State

```zin
state
  balance is an integer from 0 to 20
  status is one of open, closed
  seen is a map from worker to an integer from 0 to 20
```

One field per line: `<name> is <type>`. Any [type](types.md) may be used.
The order of fields does not change the meaning, but it is the order in
which states are printed and, for `--inductive`, the order counterexamples
are searched in.

Model what the behavior depends on and nothing else. Every field multiplies
the state space: the account above, with one `seen` entry per worker, has
1,547 reachable states instead of 22.

```console
$ zin check acct.zin
ok Bank account: 1547 states, 10761 transitions, 0 properties, 1 scenario
```

A field nothing reads is also invisible to the tests: `zin observe` reports
it as states no call can tell apart. See
[what observe can tell apart](../conformance.md#what-observe-can-tell-apart).

## Initial values

```zin
initially
  balance is 0
  status is open
  at of every worker is idle
  at of alice is holding         # single entries refine 'of every'
  orders of every customer is {status: draft, items: 0}
```

`initially` describes exactly one state. Every field, every map key and every
record member needs a value:

```console
$ zin check missing.zin
missing.zin: spec: status has no initial value; set it in the initially block
```

- **Constants only.** An initial value cannot read another field:

  ```console
  $ zin check init-expr.zin
  spec: init-expr.zin:7:3: initial values must be constants, but this reads a
  ```

- **Once per slot**, except that `of every` sets a default which single
  entries then refine. Giving the same field two values is an error:

  ```console
  $ zin check twice-init.zin
  spec: twice-init.zin:6:3: this value is already assigned in the same block
  ```

- **Interchangeable values** cannot be singled out here (`at of ann is ...`
  is an error when `worker` is interchangeable); only `of every` works.

To start from several states, start from one and add a setup action, or make
the choice a parameter of the first action.

## Actions

```zin
fair action deposit(amount is an integer from 1 to 5, w is a worker)
  requires status is open
  requires balance + amount is at most 20
  balance becomes balance + amount
  seen of w becomes balance
```

An action is a heading, optional parameters, then `requires` lines and
assignments in any order. `fair` and `strongly fair` before `action` are
promises about the environment, used only by liveness properties; see
[fairness](properties.md#fairness).

### Parameters

```zin
action set(t is a level)
action switch(m is a mode)
action deposit(amount is an integer from 1 to 5, w is a worker)
```

Parameters are integers, booleans or enumerations, inline or named. The
checker tries every combination of arguments in every state, so each
combination is a separate **call**: the thermostat's `set(t is a level)` over
`0 to 3` is four calls, `set(0)` to `set(3)`. A specification may have at most
100,000 calls.

Parameters are read-only:

```console
$ zin check noparam.zin
spec: noparam.zin:7:3: only a state field, or an entry or member of one, can be assigned
```

Parameter names are what the bindings pass to your adapter. In Python they
become keyword arguments (`def deposit(self, amount, w)`), in JavaScript keys
of one object (`deposit: ({ amount, w }) => ...`), in Go `args.Int("amount")`.
See [bindings](../bindings/index.md).

### Requires

```zin
  requires status is open
  requires balance + amount is at most 20
```

A call is **enabled** when every `requires` holds. Several lines mean the
same as one line joined with `and`, and they are evaluated in order, so an
earlier line can guard a later one:

```zin
  requires lease is not nothing
  requires left of lease is 1      # safe: lease was checked above
```

When any `requires` is false the call is **rejected**: the state does not
change. That is not an error. It is part of the specification, the tests
check that your implementation refuses the same call, and scenarios can
assert it with `reject`.

An action with no `requires` is always enabled. An action with `requires` but
no assignment is a call that is accepted without changing the state, useful
for queries and health checks:

```zin
action ping
  requires up
```

### Assignments

```zin
  balance becomes balance + amount
  seen of w becomes balance
  at of every worker becomes idle
  items of orders of c becomes items of orders of c + 1
```

- **All assignments read the state before the action.** They happen at once,
  so order does not matter, and two lines can swap values:

  ```zin
  system "Swap"
  state
    a is an integer from 0 to 2
    b is an integer from 0 to 2
  initially
    a is 1
    b is 2
  action swap
    a becomes b
    b becomes a
  scenario "Swap once"
    swap
    expect a is 2 and b is 1
  ```

  In the account above, `seen of w becomes balance` records the balance
  *before* the deposit.

- **A field not assigned keeps its value.**
- **One write per slot.** Assigning the same field, map entry or record
  member twice is an error, including a whole record and one of its members:

  ```console
  $ zin check twice.zin
  spec: twice.zin:9:3: this value is already assigned in the same block
  ```

  The one exception: `m of every <type> becomes ...` sets a default that
  assignments to single entries refine.

  ```zin
  action reset_except(w is a worker)
    at of every worker becomes idle
    at of w becomes busy
  ```

- **A path may use `of every` once**, and every other key on it must then be
  a constant.
- **Optional records and enumeration cases with members are assigned whole.**
  See [types](types.md#optional-values).
- **There is no conditional statement.** Put the condition in the value:
  `status becomes if attempts is 3 then failed else queued`. If the condition
  decides *whether* the call happens, it belongs in a `requires`, or in two
  actions.

### Model faults

An assignment that leaves its declared range, a division by zero, or a read of
an absent value is a **model fault**, reported with the shortest trace that
reaches it:

```console
$ zin check tank.zin
model: tank.zin:7:3: add: balance would become 4, but it is an integer from 0 to 3; add a 'requires' that rules this out
counterexample:
  start: balance=0
  1. add  ->  balance=1
  2. add  ->  balance=2
  3. add  ->  balance=3
```

A fault means the specification has not said what happens. Usually the real
system refuses the call, so say so with a `requires`. If it really does
something else (wraps around, saturates), write that: `balance becomes if
balance is 3 then 3 else balance + 1`.

## Determinism

Actions are deterministic: a call in a state leads to exactly one next state.
A choice the environment makes, such as whether the network delivers or a
payment is declined, is modeled as a parameter or as separate actions:

```zin
fair action answer(ok is a boolean, retryable is a boolean)
  requires status is pending
  status becomes if ok then paid else if retryable and tries < 2 then new else declined
```

The checker tries every argument, so every outcome is explored. The tests
then drive the same choice into your implementation, typically through a
stub. See [modeling patterns](patterns.md#nondeterminism-through-parameters).

## Duplicate and unused actions

An action name may appear once per `system` block:

```console
$ zin check dup.zin
spec: dup.zin:8:8: duplicate action go
```

(Across blocks, the same name means a shared step; see
[composition](composition.md).)

An action that no reachable state enables is reported as a note after a
successful check. It is almost always a mistake:

```console
$ zin check never.zin
ok Never: 3 states, 2 transitions, 0 properties, 0 scenarios
  note: action reset is never enabled in any reachable state
```
