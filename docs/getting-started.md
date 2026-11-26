---
title: Getting started
nav_order: 2
---

# Getting started
{: .no_toc }

One library book, start to finish: model it, prove it, find the rules nothing checks, then run the
proof as tests against a Python and a JavaScript implementation. Every output on this page is what
`zin` prints for the files shown.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

```sh
cargo install zin-spec            # the zin binary (Rust 1.88+)
npm install --save-dev zin-verify # Jest, Vitest, node:test, Mocha, Playwright
go get github.com/enekos/zin/bindings/go
pip install /path/to/zin/bindings/python   # pytest / unittest, from a checkout
```

Prebuilt `zin` binaries for Linux and macOS (x86_64, arm64) are attached to each
[GitHub release](https://github.com/enekos/zin/releases). You need the binary to check
specifications; test machines can do without it if you [commit the plan](#commit-the-plan).

## 1. Describe the states

A member borrows a book, may renew it twice, and gives it back. While it is out, other members can
reserve it; whoever reserved it gets it next.

Start with what the system *is* at any moment. Every field has a finite type:

```zin
system "Library loan"

types
  member is one of ana, ben

state
  holder is a member or nothing
  renewals is an integer from 0 to 2
  waiting is a set of member

initially
  holder is nothing
  renewals is 0
  waiting is []
```

- `member` is a named enumeration, declared once and used as a type everywhere.
- `a member or nothing` is an [optional value](language/types.md): no made-up "nobody" member.
- `a set of member` holds any selection of the members.
- `initially` gives every field exactly one starting value.

Two members are enough. Zin explores every combination of values, so small ranges find the same
defects as large ones, much faster. See [modeling patterns](language/patterns.md).

## 2. Describe the calls

Each `action` is a call the outside world can make. `requires` lines say when it is allowed;
`becomes` lines say what changes. A call whose `requires` fail is **rejected** and changes nothing,
and rejections are as much part of the contract as acceptances.

```zin
action borrow(m is a member)
  requires holder is nothing
  holder becomes m

action renew
  requires holder is not nothing
  requires waiting is []
  requires renewals < 2
  renewals becomes renewals + 1

action give_back
  requires holder is not nothing
  holder becomes nothing
  renewals becomes 0

action reserve(m is a member)
  requires holder is not nothing
  requires holder is not m
  requires not waiting has m
  waiting becomes waiting with m
```

Parameters range over finite types too, so `borrow(ana)` and `borrow(ben)` are both tried in every
state.

## 3. Say what must hold

Properties are claims about every reachable state:

```zin
always
  holder is nothing implies renewals is 0
  holder is nothing or not waiting has holder

sometimes
  renewals is 2

scenario "A reservation blocks renewal"
  borrow(ana)
  renew
  reserve(ben)
  reject renew
  expect renewals is 1
```

- `always` must hold in every reachable state. The second line says the holder is never waiting
  for their own book.
- `sometimes` must hold in at least one. It guards against a model so strict that nothing
  interesting can happen.
- A `scenario` is a worked example: each call must be accepted, each `reject` refused, each
  `expect` true.

## 4. Check it

Put the three parts in `loan.zin` and check it:

```console
$ zin check loan.zin
violation: loan.zin:18:3: 'always holder is nothing or not waiting has holder' is violated
counterexample:
  start: holder=nothing, renewals=0, waiting=[]
  1. borrow(ana)  ->  holder=ana
  2. reserve(ben)  ->  waiting=[ben]
  3. give_back  ->  holder=nothing
  4. borrow(ben)  ->  holder=ben
```

Ben reserved the book, got it, and is still on the waiting list. The counterexample is the
*shortest* sequence of calls that breaks the property, because exploration is breadth-first. Each
step shows only the fields that changed.

The fix belongs in the model: a reservation is used up when it is served, and while anyone is
waiting, only someone who is waiting may borrow.

```zin
action borrow(m is a member)
  requires holder is nothing
  requires waiting is [] or waiting has m
  holder becomes m
  waiting becomes waiting without m
```

```console
$ zin check loan.zin
ok Library loan: 15 states, 26 transitions, 3 properties, 1 scenario
```

15 states, 26 transitions: every one of them satisfies both `always` lines, one of them has
`renewals is 2`, and the scenario plays out.

## 5. Say what must eventually happen

Safety says nothing bad happens. Liveness says something good does. A reservation should mean the
book comes back:

```zin
whenever waiting is not [] eventually holder is nothing
```

```console
$ zin check loan.zin
ok Library loan: 15 states, 26 transitions, 4 properties, 1 scenario
```

It holds because, with someone waiting, `renew` is refused and `reserve` runs out of members: the
only thing left to do is give the book back. Swap it, for a moment, for a claim that does not hold:

```zin
eventually waiting is not []
```

```console
$ zin check loan.zin
violation: loan.zin:23:1: 'eventually waiting is not []' fails: the system can run forever without it happening
counterexample:
  start: holder=nothing, renewals=0, waiting=[]
  then forever:
  1. borrow(ana)  ->  holder=ana
  2. give_back  ->  holder=nothing
```

A liveness counterexample is a loop: nobody is ever obliged to reserve. Put the `whenever` line back.
[Properties](language/properties.md) covers liveness, fairness and deadlock in depth.

## 6. Find the rules nothing checks

A specification that allows everything proves nothing. `zin audit` changes the specification one
rule at a time (drops a `requires`, flips a comparison, shifts a constant) and reports the changes
no property or scenario notices. Trimmed to the survivors:

```console
$ zin audit loan.zin
SURVIVED line 27: action borrow: drop 'requires waiting is [] or waiting has m' (no property or scenario notices)
  borrow(ana)
  reserve(ben)
  give_back
  borrow(ana) → original rejected, mutant accepted, holder=ana, renewals=0, waiting=[ben]
SURVIVED line 38: action give_back: drop 'requires holder is not nothing' (no property or scenario notices)
  give_back → original rejected, mutant accepted, holder=nothing, renewals=0, waiting=[]
SURVIVED line 43: action reserve: drop 'requires holder is not nothing' (no property or scenario notices)
  reserve(ana) → original rejected, mutant accepted, holder=nothing, renewals=0, waiting=[ana]
...
33 behavior-changing mutants: 30 caught, 3 survived (0 equivalent, 0 skipped)
```

Each survivor comes with the shortest run where the original and the mutant disagree. The first one
says: nothing pins down that Ana cannot jump the queue. That rule matters, so pin it with a
scenario, `reject` lines included:

```zin
scenario "Only the holder's book can come back"
  reject give_back
  reject reserve(ana)
  borrow(ana)
  reject borrow(ben)
  reserve(ben)
  reject reserve(ben)
  give_back
  expect holder is nothing
  reject borrow(ana)
  borrow(ben)
```

```console
$ zin audit loan.zin
...
33 behavior-changing mutants: 33 caught, 0 survived (0 equivalent, 0 skipped)
```

`zin audit` exits `1` while a mutant survives, so it can gate CI. More in
[auditing and observability](guides/audit-and-observe.md).

<details markdown="1">
<summary>The complete <code>loan.zin</code></summary>

```zin
system "Library loan"

types
  member is one of ana, ben

state
  holder is a member or nothing
  renewals is an integer from 0 to 2
  waiting is a set of member

initially
  holder is nothing
  renewals is 0
  waiting is []

always
  holder is nothing implies renewals is 0
  holder is nothing or not waiting has holder

sometimes
  renewals is 2

whenever waiting is not [] eventually holder is nothing

action borrow(m is a member)
  requires holder is nothing
  requires waiting is [] or waiting has m
  holder becomes m
  waiting becomes waiting without m

action renew
  requires holder is not nothing
  requires waiting is []
  requires renewals < 2
  renewals becomes renewals + 1

action give_back
  requires holder is not nothing
  holder becomes nothing
  renewals becomes 0

action reserve(m is a member)
  requires holder is not nothing
  requires holder is not m
  requires not waiting has m
  waiting becomes waiting with m

scenario "A reservation blocks renewal"
  borrow(ana)
  renew
  reserve(ben)
  reject renew
  expect renewals is 1

scenario "Only the holder's book can come back"
  reject give_back
  reject reserve(ana)
  borrow(ana)
  reject borrow(ben)
  reserve(ben)
  reject reserve(ben)
  give_back
  expect holder is nothing
  reject borrow(ana)
  borrow(ben)
```

</details>

## 7. Derive the tests

The model is proven. Now it becomes the oracle for real code. `zin plan` walks the state graph and
emits call sequences that take every transition and try every refused call in every state:

```console
$ zin plan loan.zin --out loan.plan.json
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
plan: 4 tests, 52 accepted steps, 64 rejected steps (cover: transitions) -> loan.plan.json
```

Four tests: your two scenarios, a witness that reaches `sometimes renewals is 2`, and one generated
*tour* that covers everything else. You never edit the plan; bindings read it. The
[plan format](plan-format.md) documents it for anyone writing a binding.

## 8. Connect it to Python

`zin init` prints a test skeleton with one method per action:

```console
$ zin init pytest loan.zin
import zin


@zin.verify("loan.zin")
class LibraryLoanConformance:
    # Exception types that mean "the implementation refused this action".
    rejects = (ValueError,)

    def setup(self):
        # Runs before every generated test: build a fresh implementation.
        self.impl = None

    def borrow(self, m):
        raise NotImplementedError("perform borrow on self.impl")
...
```

Here is an implementation, `library.py`, with a bug the model already rules out:

```python
class Book:
    def __init__(self):
        self.holder = None
        self.renewals = 0
        self.waiting = []

    def borrow(self, member):
        if self.holder is not None:
            raise ValueError("already on loan")
        if self.waiting and member not in self.waiting:
            raise ValueError("reserved for someone else")
        self.holder = member
        if member in self.waiting:
            self.waiting.remove(member)

    def renew(self):
        if self.holder is None:
            raise ValueError("not on loan")
        if self.renewals >= 2:               # forgets the waiting list
            raise ValueError("too many renewals")
        self.renewals += 1

    def give_back(self):
        if self.holder is None:
            raise ValueError("not on loan")
        self.holder = None
        self.renewals = 0

    def reserve(self, member):
        if self.holder is None or self.holder == member or member in self.waiting:
            raise ValueError("cannot reserve")
        self.waiting.append(member)
```

The adapter, `test_loan.py`, maps each action to a method and says how the implementation refuses:

```python
import zin
from library import Book


@zin.verify("loan.zin")
class LoanConformance:
    rejects = (ValueError,)

    def setup(self):
        self.book = Book()

    def borrow(self, m):
        self.book.borrow(m)

    def renew(self):
        self.book.renew()

    def give_back(self):
        self.book.give_back()

    def reserve(self, m):
        self.book.reserve(m)

    def observe(self):
        return {
            "holder": self.book.holder,
            "renewals": self.book.renewals,
            "waiting": set(self.book.waiting),
        }
```

- Arguments arrive by parameter name; enum values as their name in the specification (`"ana"`).
- `None` is how Python observes `nothing`; a Python `set` matches a set field in any order.
- `observe` is optional and may return any subset of the fields. With it, wrong state is caught on
  the step that causes it.

```console
$ pytest -q
FAILED test_loan.py::LoanConformance::test_1_scenario_a_reservation_blocks_renewal
FAILED test_loan.py::LoanConformance::test_4_tour_1
```

```text
Library loan: implementation diverges from the specification in test "scenario: A reservation blocks renewal" at step 4
the implementation accepted renew, but the specification rejects it
steps taken:
  1. borrow(ana)
  2. renew
  3. reserve(ben)
  4. reject renew
spec fingerprint: ec794a619ea0a06b
scenario "Replay divergence"
  borrow(ana)
  renew
  reserve(ben)
  reject renew
```

The `scenario "Replay divergence"` block is ready to paste into `loan.zin` if you want to keep the
case. Fix `renew` to refuse while anyone is waiting (`if self.waiting or self.renewals >= 2`):

```console
$ pytest -v
test_loan.py::LoanConformance::test_1_scenario_a_reservation_blocks_renewal PASSED [ 25%]
test_loan.py::LoanConformance::test_2_scenario_only_the_holder_s_book_can_come_back PASSED [ 50%]
test_loan.py::LoanConformance::test_3_reaches_sometimes_renewals_is_2 PASSED [ 75%]
test_loan.py::LoanConformance::test_4_tour_1 PASSED                      [100%]
```

Every test is an ordinary pytest test: `-k`, `-x`, xdist and reports work as usual. The
[Python binding](bindings/python.md) page covers the rest.

## 9. Check what the tests can see

A test sees whether each call was accepted and whatever `observe` returns. Suppose the adapter
observed only `holder` and `waiting`:

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
...
6 states in 2 blind groups cannot be told apart
```

With someone waiting, `renew` is refused whatever the count, and giving the book back resets it. An
implementation that counted renewals wrong there would pass every test. Observing `renewals` too
closes the gap:

```console
$ zin observe loan.zin --fields holder,renewals,waiting
ok Library loan: 15 states, 26 transitions, 4 properties, 2 scenarios
observing holder, renewals, waiting: 15 states show 15 distinct observations
default plan: 0 of 26 transitions could reach a look-alike state unnoticed; zin plan --observe holder,renewals,waiting adds 0 tests (0 steps)
every state can be told apart
```

## 10. The same model, in JavaScript

The plan is data, so every binding runs the same tests. With `node:test`:

```js
import { describe, test } from "node:test";
import { verify } from "zin-verify";
import { Book } from "./library.mjs";

verify(new URL("./loan.zin", import.meta.url), () => {
  const book = new Book();
  return {
    actions: {
      borrow: ({ m }) => book.borrow(m),
      renew: () => book.renew(),
      give_back: () => book.giveBack(),
      reserve: ({ m }) => book.reserve(m),
    },
    rejects: (error) => error instanceof RangeError,
    observe: () => ({ holder: book.holder, renewals: book.renewals, waiting: new Set(book.waiting) }),
  };
}, { describe, test });
```

```console
$ node --test loan.test.mjs
  ✔ scenario: A reservation blocks renewal
  ✔ scenario: Only the holder's book can come back
  ✔ reaches: sometimes renewals is 2
  ✔ tour 1
✔ Library loan conforms to loan.zin
```

Jest and Mocha pick up their global `describe`/`test`, so the third argument can go. See
[JavaScript and TypeScript](bindings/javascript.md), or [Go](bindings/go.md), [Rust](bindings/rust.md)
and [JUnit 5](bindings/junit.md).

## Commit the plan

Bindings find the plan in this order: a plan you pass explicitly, a fresh `loan.plan.json` next to
the specification, or the `zin` binary on `ZIN_BIN` or `PATH`. Commit the plan and test machines need
nothing but the binding:

```sh
zin plan loan.zin --out loan.plan.json
git add loan.zin loan.plan.json
```

The plan records a fingerprint of the specification. Edit `loan.zin` without regenerating it and the
tests fail with the command to run, never with a stale pass.

## Where next

- [Language](language/index.md): every construct, with runnable examples.
- [Modeling patterns](language/patterns.md): servers, retries, queues, locks, timeouts.
- [Bindings](bindings/index.md): adapters in every framework.
- [Continuous integration](guides/ci.md): check, audit and test on every push.
