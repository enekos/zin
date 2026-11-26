---
title: Home
nav_order: 1
description: Zin specifications read like a behavioral contract. Zin proves them over every reachable state, then runs them as tests against your code.
---

# Zin
{: .fs-9 }

Write the behavior once, in words. Prove it. Then let it test your code.
{: .fs-6 .fw-300 .hero-tagline }

[Get started](getting-started.md){: .btn .btn-primary .fs-5 .mb-4 .mb-md-0 .mr-2 }
[Learn the language](language/index.md){: .btn .fs-5 .mb-4 .mb-md-0 .mr-2 }
[Pick a binding](bindings/index.md){: .btn .fs-5 .mb-4 .mb-md-0 }

---

**Zin** is Basque for a solemn promise. A Zin specification describes a system as states and the
calls that change them, in plain English, and the `zin` tool does three things with it:

1. **Proves it.** Every reachable state is explored. Safety, liveness and deadlock properties either
   hold for all of them or you get the shortest counterexample.
2. **Turns it into tests.** From the proven model, Zin derives a plan that tries every transition and
   every refusal, and replays it inside the test framework you already use: pytest, unittest, Jest,
   Vitest, `node:test`, Mocha, Playwright, `go test`, `cargo test` and JUnit 5.
3. **Keeps it honest.** `zin audit` mutates the specification to show which rules nothing checks,
   `zin observe` shows which states your tests cannot tell apart, and `zin trace` checks logs of real
   executions.

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

action withdraw(amount is an integer from 1 to 5)
  requires status is open
  requires amount is at most balance
  balance becomes balance - amount

action close
  requires status is open
  requires balance is 0
  status becomes closed

action reopen
  requires status is closed
  status becomes open
```

```console
$ zin check account.zin
ok Bank account: 22 states, 182 transitions, 1 property, 0 scenarios
```

## From proof to tests

22 states and 182 transitions is not something you test by hand. You write one adapter that maps each
action onto your code, and the binding registers the derived plan as ordinary tests:

```python
import zin
from bank import Account

@zin.verify("account.zin")
class AccountConformance:
    rejects = (ValueError,)          # how the implementation says "no"

    def setup(self):
        self.account = Account()

    def deposit(self, amount):
        self.account.deposit(amount)

    def withdraw(self, amount):
        self.account.withdraw(amount)

    def close(self):
        self.account.close()

    def reopen(self):
        self.account.reopen()

    def observe(self):               # optional: compare state after every step
        return {"balance": self.account.balance, "status": self.account.status}
```

An implementation whose limit is 21 instead of 20 fails with the shortest way to see it:

```text
Bank account: implementation diverges from the specification in test "tour 1" at step 63
the implementation accepted deposit(5), but the specification rejects it
...
shortest reproduction:
  1. deposit(1)
  2. deposit(5)
  3. deposit(5)
  4. deposit(5)
  5. reject deposit(5)
```

The same adapter exists for [JavaScript and TypeScript](bindings/javascript.md),
[Playwright](bindings/playwright.md), [Go](bindings/go.md), [Rust](bindings/rust.md) and
[JUnit 5](bindings/junit.md).

## What a check proves

| You write | Zin proves, over every reachable state |
| --- | --- |
| `always <condition>` | the condition holds in all of them |
| `never <condition>` | it holds in none of them |
| `sometimes <condition>` | at least one satisfies it (the model is not vacuous) |
| `eventually <condition>` | every execution reaches it |
| `whenever <p> eventually <q>` | after any state where `p` holds, `q` always follows |
| `always possible <condition>` | from every state there is still a way to reach it |
| `terminal when <condition>` | the system only stops where you said it may; anything else is a deadlock |
{: .proof-table }

Exploration is exhaustive within the declared finite domains. Hitting the state limit is an error,
never a silent partial pass. See [properties](language/properties.md).

## Where to go next

| If you want to | Read |
| --- | --- |
| Write a first specification and run it against code, start to finish | [Getting started](getting-started.md) |
| Learn every construct of the language, with runnable examples | [Language](language/index.md) |
| Connect a specification to your test framework | [Bindings](bindings/index.md) |
| Find the rules nothing checks, or the states tests cannot see | [Auditing and observability](guides/audit-and-observe.md) |
| Check logs of real executions | [Checking recorded executions](guides/traces.md) |
| Run it all in CI | [Continuous integration](guides/ci.md) |
| Look up a command, flag or diagnostic code | [Reference](reference/index.md) |

## Scope

Zin models finite-state systems: bounded integers, booleans and enums, and records, maps, optional
values, sets, bounded lists and enumerations with members built from them. It does not model unbounded
data, real time, probabilities or floating point. Conformance testing shows that an implementation
agrees with the model on every modeled transition; it is exhaustive over the model, not a proof about
the implementation's code. [Design](design.md) explains what is proven, what is not, and why.
