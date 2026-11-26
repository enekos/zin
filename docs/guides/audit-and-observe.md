---
title: Auditing and observability
parent: Guides
nav_order: 1
---

# Auditing and observability
{: .no_toc }

Conformance tests are only as strong as two things: the specification they come from, and what the
adapter lets them see. `zin audit` measures the first, `zin observe` the second.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## `zin audit`: is the specification pinned down?

A specification is a list of rules (`requires` lines and assignments) plus claims about them
(properties and scenarios). A rule that no claim depends on can be deleted or changed without anything
failing, and then the derived tests will happily accept an implementation that breaks it, because the
model says it is fine.

`zin audit` finds those rules with mutation testing, applied to the specification instead of the code.
It makes one small change at a time, re-checks the result, and sorts each change into one of four
outcomes.

### The changes it makes

| Change | Example |
| --- | --- |
| Drop a `requires` | `drop 'requires holder is nothing'` |
| Drop an assignment | `drop 'holder becomes nothing'` |
| Swap a comparison | `'is' becomes 'is not'`, `'<' becomes '<='`, `'>=' becomes '>'` |
| Swap a connective | `'and' becomes 'or'`, `'or' becomes 'and'` |
| Swap arithmetic | `'+' becomes '-'`, `'-' becomes '+'` |
| Swap a set or list operation | `'with' becomes 'without'` and back |
| Shift a constant | `2 becomes 3`, `2 becomes 1` |

At most 500 mutants are tried per run, in source order.

### What each outcome means

| Printed | Meaning | What to do |
| --- | --- | --- |
| `caught` | A property, a scenario or a declared range fails on the mutant. The reason is in parentheses. | Nothing. |
| `SURVIVED` | Reachable behavior changed and nothing noticed. The shortest run where the two differ follows. | Add a property or a scenario (often one `reject` line), or delete the rule if it is not wanted. |
| `same` | Reachable behavior is unchanged: the rule is implied by the others. | Usually fine. Sometimes a hint that the rule is redundant. |
| skipped | The mutant does not type-check, or hit an exploration limit. | Nothing. |

The summary counts only behavior-changing mutants:

```console
$ zin audit examples/delivery.zin
...
same     line 26: action ship: drop 'requires paid' (reachable behavior is unchanged)
...
16 behavior-changing mutants: 16 caught, 0 survived (1 equivalent, 0 skipped)
```

`drop 'requires paid'` is `same` because `ship` already requires the order to be packed, and packing
requires payment: the rule is redundant, and the audit says so without failing.

### Reading a survivor

```console
SURVIVED line 27: action borrow: drop 'requires waiting is [] or waiting has m' (no property or scenario notices)
  borrow(ana)
  reserve(ben)
  give_back
  borrow(ana) → original rejected, mutant accepted, holder=ana, renewals=0, waiting=[ben]
```

The indented lines are the shortest sequence of calls on which the original and the mutant agree,
followed by the first call where they do not, with both outcomes. That sequence is almost a scenario
already:

```zin
scenario "Reservations are served first"
  borrow(ana)
  reserve(ben)
  give_back
  reject borrow(ana)
```

Paste it, run the audit again, and the survivor becomes `caught`. Prefer a property when the rule is
a general truth (`always holder is nothing or not waiting has holder`), and a scenario when it is one
specific case you want written down.

### Exit codes

`zin audit` exits `1` while any mutant survives and `0` when none does, so it can gate a pull request.
See [continuous integration](ci.md).

{: .tip }
Liveness properties catch mutants too. In the [getting started](../getting-started.md) example,
`whenever waiting is not [] eventually holder is nothing` alone catches `drop 'holder becomes nothing'`
from `give_back`, because the book would then never come back.

## `zin observe`: can the tests see the state?

A test run sees two things after each step: whether the call was accepted, and the fields `observe`
returns. Two model states that agree on both are told apart only by a later call that one accepts and
the other refuses, or that leads them to different observations. Sometimes no call ever does.

`zin observe` computes this for a set of fields:

```console
$ zin observe loan.zin --fields holder,waiting
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
observing holder, waiting: 15 states show 7 distinct observations
states that look alike are told apart by at most 2 further calls, for example
  holder=ana, renewals=0, waiting=[]
  holder=ana, renewals=1, waiting=[]
  by: renew, renew
BLIND 3 states that no sequence of calls tells apart, for example
  holder=ana, renewals=0, waiting=[ben]
  holder=ana, renewals=1, waiting=[ben]
  they differ in renewals: observe it, or the specification does not need it
BLIND 3 states that no sequence of calls tells apart, for example
  holder=ben, renewals=0, waiting=[ana]
  holder=ben, renewals=1, waiting=[ana]
  they differ in renewals: observe it, or the specification does not need it
default plan: 0 of 26 transitions could reach a look-alike state unnoticed; zin plan --observe holder,waiting adds 0 tests (0 steps)
6 states in 2 blind groups cannot be told apart
```

Line by line:

- **15 states show 7 distinct observations.** Looking only at `holder` and `waiting`, 15 states
  collapse into 7.
- **Told apart by at most 2 further calls.** Where look-alike states can be separated, this is the
  longest it takes, with an example: after one renewal, a second `renew` is still accepted only if the
  first state had 0.
- **BLIND.** States no sequence of calls can ever separate. Here, with someone waiting, `renew` is
  refused whatever the count and giving the book back resets it. An implementation that counted
  renewals wrong there passes every possible test. The fix is to observe one of the fields they differ
  in, or, if nothing depends on the field, to drop it from the specification.
- **Default plan.** How many transitions could land in a look-alike state without the default plan
  noticing, and how many tests `--observe` would add to close that gap.

`zin observe` exits `1` when there are blind states.

### Without `observe` at all

Leave out `--fields` to ask about an adapter that has no `observe`: only accepted and rejected calls.

```console
$ zin observe loan.zin
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
observing no fields (accepted and rejected calls only): 15 states show 1 distinct observation
...
default plan: 11 of 26 transitions could reach a look-alike state unnoticed; zin plan --observe '' adds 11 tests (51 steps)
6 states in 2 blind groups cannot be told apart
```

### Closing the look-alike gap: `zin plan --observe`

The default plan takes every transition once, but what comes after it in the tour may not separate the
state reached from one that looks the same. An implementation whose step lands in the wrong look-alike
state then passes. `--observe` adds *identify* tests: the shortest setup, the transition, then the
shortest calls that every look-alike state would answer differently.

```console
$ zin plan examples/editor.zin --observe draft,status --out editor.plan.json
ok Quantity editor: 36 states, 96 transitions, 5 properties, 4 scenarios
plan: 56 tests, 595 accepted steps, 116 rejected steps (cover: transitions; observing draft, status: 48 identify tests for 53 of 96 transitions) -> editor.plan.json
```

The identify tests assume the adapter returns exactly the named fields. Commit the plan (`--out`) so
every runner uses it. `--observe ''` plans for an adapter without `observe`.

The guarantee is the one from state identification in finite-state testing: a single transition that
lands in another *specified* state is caught. An implementation with extra states the specification
does not have is outside it.

## Choosing coverage

Audit and observe make the specification and the adapter strong. `--cover` decides how much of the
model the plan walks.

| Cover | The plan contains | Use it for |
| --- | --- | --- |
| `transitions` (default) | every transition, and every refused call in every state | the normal case |
| `states` | a visit to every reachable state | slow implementations: browsers, databases |
| `actions` | every action accepted once and, where possible, refused once | smoke tests |
| `scenarios` | only your scenarios | a fast first pass |
| `pairs` | every pair of adjacent calls from every state, refusals included | implementations that keep history the model does not |

`transitions` tries each call once per model state. An implementation that keeps history the model
does not, such as an `ack` that accepts the first acknowledgement and rejects the second, passes it:
the model is in the same state both times. `pairs` makes each (state, first call, second call) an
obligation. `--pair-limit N` samples a deterministic subset, repetitions first.

`--max-steps` (default 200) is a soft target for tour length and `--step-budget` (default and maximum
2,000,000) a hard ceiling on every emitted step. A plan that cannot fit the budget exits with a limit
error instead of silently covering less.

The number of steps is roughly `states × calls`. When that is too many for a slow implementation,
shrink the model's ranges for testing, use `states`, or run `transitions` nightly.

## What none of this shows

- Conformance is exhaustive over the **model**, not the code. An implementation that misbehaves for
  `amount = 6` when the model stops at 5 is not exercised there.
- Each test drives the implementation **sequentially**. Interleavings are explored in the model;
  conformance checks that each atomic step matches. Record real concurrent runs and use
  [`zin trace`](traces.md).
- Liveness is proven about the model. A test cannot show that an implementation eventually does
  something.
