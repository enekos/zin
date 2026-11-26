---
title: Scenarios
parent: Language
nav_order: 5
---

# Scenarios
{: .no_toc }

A scenario is a worked example: a sequence of calls from the initial state,
with the refusals and states you expect along the way. Properties say what
must hold everywhere; scenarios say what one particular run looks like.

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Syntax

```zin
system "Counter"
state
  n is an integer from 0 to 3
initially
  n is 0
action inc(by is an integer from 1 to 2)
  requires n + by <= 3
  n becomes n + by
action clear
  requires n > 0
  n becomes 0
scenario "Count up"
  reject clear
  inc(1)
  inc with by = 2
  expect n is 3
  reject inc(1)
  clear
  expect n is 0
```

```console
$ zin check counter.zin
ok Counter: 4 states, 8 transitions, 0 properties, 1 scenario
```

| Line | Means |
| --- | --- |
| `inc(1)` | this call must be enabled; take it |
| `inc with by = 2` | the same, with named arguments |
| `clear` | an action without parameters |
| `reject clear` | this call must be refused here; the state does not change |
| `expect n is 3` | this condition must hold now |

Every scenario starts from the initial state. Lines run in order. Positional
arguments follow the order of the action's parameters; named arguments
(`set with y = true, x = 2`) may come in any order.

By convention scenarios go at the end of a `system` block, after the
actions (the checker does not enforce an order). In a
[composition](composition.md) they usually sit in the stateless block that
names the whole.

## What a failing scenario looks like

A call that the specification refuses:

```console
$ zin check counter.zin
violation: counter.zin:17:3: scenario "Count up": inc(1) is not allowed here; a 'requires' rejects it
counterexample:
  start: n=0
  1. inc(1)  ->  n=1
  2. inc(2)  ->  n=3
```

A `reject` for a call that the specification allows:

```console
$ zin check counter.zin
violation: counter.zin:18:3: scenario "Count up": expected clear to be rejected, but the specification allows it
counterexample:
  start: n=0
  1. inc(1)  ->  n=1
  2. inc(2)  ->  n=3
```

An `expect` that does not hold:

```console
$ zin check counter.zin
violation: counter.zin:16:3: scenario "Count up": expected n is 2
counterexample:
  start: n=0
  1. inc(1)  ->  n=1
  2. inc(2)  ->  n=3
```

Mistakes in the call itself are reported before anything runs:

```console
$ zin check counter.zin
spec: counter.zin:14:3: argument by of inc must be an integer from 1 to 2, but 5 was given
$ zin check counter.zin
spec: counter.zin:14:3: inc takes 1 argument(s), but 0 were given
$ zin check counter.zin
spec: counter.zin:14:3: unknown action increment
  = see: zin explain name.unknown
```

## Why write scenarios

**Documentation that cannot rot.** A scenario is checked on every `zin
check`. If someone changes a `requires` and the example no longer works, the
check fails.

**Pinning the specification.** Properties leave room: many specifications
satisfy `always balance <= 20`. A scenario pins exact behavior, which is what
`zin audit` measures. A single `reject` line often kills a surviving mutant:
if dropping `requires not waiting has m` goes unnoticed, add

```zin
  reserve(ben)
  reject reserve(ben)
```

**The first tests.** Every derived plan starts with the scenarios, in order,
each as its own test, before the generated tours:

```console
$ zin plan counter.zin --out counter.plan.json
ok Counter: 4 states, 8 transitions, 0 properties, 1 scenario
plan: 2 tests, 11 accepted steps, 4 rejected steps (cover: transitions) -> counter.plan.json
```

```json
"tests": [
  {"name": "scenario: Count up", "kind": "scenario", "steps": [...]},
  {"name": "tour 1", "kind": "tour", "steps": [...]}
]
```

In pytest the scenario is `test_1_scenario_count_up`; in Jest, Vitest and
Playwright it is a test named `scenario: Count up`. You can run it alone with
the framework's own filter (`-k`, `-t`, `--grep`, `go test -run`). With
`--cover scenarios`, the plan contains only your scenarios.

**Diagrams.** `zin diagram spec.zin "Count up"` draws a scenario as a Mermaid
sequence diagram:

```console
$ zin diagram counter.zin "Count up"
sequenceDiagram
    participant Client
    participant System as Counter
    Note over System: n=0
    Client-xSystem: clear (rejected)
    Client->>System: inc(1)
    Note over System: n=1
    Client->>System: inc(2)
    Note over System: n=3
    Note right of System: expect n is 3
    Client-xSystem: inc(1) (rejected)
    Client->>System: clear
    Note over System: n=0
    Note right of System: expect n is 0
```

## From a failing test to a scenario

When an implementation diverges from the specification, every binding prints
the failing prefix as a scenario, ready to paste:

```text
spec fingerprint: 09d83aa6c856ea87
scenario "Replay divergence"
  borrow(ana)
  renew
  reserve(ben)
  reject renew
```

Paste it into the specification (rename it), and the bug you just found is
pinned as a worked example and as the first test of every future plan.

## Tips

- One scenario per behavior a reviewer would ask about: the happy path, each
  refusal that matters, the retry, the expiry.
- End with `expect` lines on the fields that matter, and with `reject` lines
  for the calls that must now be refused.
- Scenarios may name [interchangeable](types.md#interchangeable-values)
  values; they describe one concrete run.
- Keep a scenario short. The generated tours cover the combinations; a
  scenario is for the case a person needs to see.
