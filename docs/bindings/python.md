---
title: "Python: pytest and unittest"
parent: Bindings
nav_order: 1
---

# Python: pytest and unittest
{: .no_toc }

`zin` (distribution `zin-verify`) turns an adapter class into a
`unittest.TestCase` with one test method per plan test, so pytest and
`python -m unittest` collect, filter and report it natively. Python 3.9+, no
dependencies.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

```sh
pip install /path/to/zin/bindings/python   # not on PyPI yet: install from a checkout
cargo install zin-spec                      # the zin binary, unless you commit plans
```

The binding runs `zin plan` at import time of the test module, or reads a
committed `<spec>.plan.json`; see [where the plan comes from](index.md#where-the-plan-comes-from).

## Minimal example

```python
import zin
from bank import Account

@zin.verify("account.zin")          # relative to this file
class AccountConformance:
    rejects = (ValueError,)          # how the implementation says no

    def setup(self):
        self.account = Account()

    def deposit(self, amount):
        self.account.deposit(amount)

    def observe(self):
        return {"balance": self.account.balance}
```

```console
$ pytest test_account.py
```

`zin init pytest account.zin` (or `zin init unittest`) prints this skeleton for
your specification, with one stub per action.

## The adapter class

The decorated class is not a `TestCase` itself; it is your adapter. The
decorator builds a new `TestCase` subclass with the same name, and for every
plan test it instantiates your class afresh.

| Member | Required | Meaning |
| --- | --- | --- |
| one method per action | yes | named like the action; arguments by parameter name |
| `rejects` | no | a tuple of exception types that mean "refused" (default: only `zin.Rejected`) |
| `setup(self)` | no | called after `__init__`, before the first step of each plan test, and again before the shortest-reproduction replay |
| `teardown(self)` | no | called after each run, also when it fails or raises |
| `observe(self)` | no | returns a `dict` of any subset of state fields, or `None` to compare nothing |

The generated `TestCase` has two class attributes: `adapter` (your class) and
`plan` (the loaded plan as a `dict`).

### How arguments arrive

Arguments are keyword arguments named after the specification's parameters:
`deposit(amount is an integer from 1 to 5)` calls `deposit(amount=3)`.
Integers arrive as `int`, booleans as `bool`, enum values as `str`.

A name that is a Python keyword gets a trailing underscore. An action `pass`
calls the method `pass_`; a parameter `from` arrives as `from_`:

```python
def pass_(self, from_):
    ...
```

{: .warning }
`zin init pytest` prints such names unchanged, which is not valid Python; add
the underscore by hand.

### Methods are synchronous

An adapter method that returns a coroutine or a generator fails the test with:

```text
LoanConformance.borrow returned coroutine without running its body; adapter methods must be synchronous, so drive async code inside the method, e.g. with asyncio.run()
```

Drive async code inside the method:

```python
def deposit(self, amount):
    asyncio.run(self.client.deposit(amount))
```

or keep one event loop per test in `setup` and call `loop.run_until_complete`.

### How the implementation says no

A call is **rejected** when the method:

- raises `zin.Rejected`, or
- raises an instance of a type listed in `rejects`, or
- returns `False` (exactly `False`; `None`, `0` and `""` count as accepted).

Any other exception propagates and fails the test as an error, with a note
listing the steps taken so far. It is never counted as a rejection. Keep
`rejects` narrow: `rejects = (Exception,)` would turn a `KeyError` in your
code into a "refusal" and hide the bug.

A rejected call must leave the implementation's state unchanged: after a
rejection the binding observes again and compares with the unchanged model
state.

### What observe may return

`observe()` returns a `dict` keyed by state field names. Only the keys present
are compared; a key that is not a field of the specification fails the test:

```text
observe() returned 'balanse', which is not a state field of the specification
```

Values are normalized first:

| Python value | Compared as |
| --- | --- |
| `int`, `bool`, `str` | themselves; `True` is not `1`, `False` is not `0` |
| `None` | nothing (an optional without a value) |
| `enum.Enum` member | its value if that is a `str`, else its name |
| `dict` | a map (keys as text: `True` → `"true"`) or a record; may be partial |
| dataclass instance, named tuple | a record of its fields |
| `set`, `frozenset` | a set, in any order |
| `list`, `tuple` | a list, in order |

Observe a set field as a Python `set`. A `list` observed for a set field is
compared in order (the order the element type declares its values).

## Lifecycle

For each plan test the binding:

1. instantiates your class and calls `setup()`;
2. compares `observe()` with the initial state;
3. for each step, calls the action, classifies the result, and compares
   `observe()` with the model state;
4. calls `teardown()` in a `finally`.

On a divergence it does it all once more, on a new instance, for the shortest
path to the failing state. A failure is raised as `zin.Divergence`, a subclass
of `AssertionError`, so pytest shows it as a test failure, not an error.

## Running and filtering

Test methods are named `test_<n>_<slug>` with the number zero-padded to the
count of tests:

```console
$ pytest -v test_loan.py
test_loan.py::LoanConformance::test_1_scenario_a_reservation_blocks_renewal PASSED
test_loan.py::LoanConformance::test_2_scenario_only_the_holder_s_book_can_come_back PASSED
test_loan.py::LoanConformance::test_3_reaches_sometimes_renewals_is_2 PASSED
test_loan.py::LoanConformance::test_4_tour_1 PASSED
```

So `-k scenario`, `-k tour`, `-x`, `--lf` and pytest-xdist work as usual.
Under unittest:

```console
$ python -m unittest -v test_loan
```

Each method's docstring is `<system>: <plan test name>`, which unittest's
verbose output prints.

## Planning options

`zin.verify`, `zin.check` and `zin.load_plan` take the options of `zin plan` as
keyword arguments:

| Argument | Type | Default | Meaning |
| --- | --- | --- | --- |
| `cover` | `str` | zin's (`"transitions"`) | `transitions`, `states`, `actions`, `scenarios`, `pairs` |
| `max_steps` | positive `int` | zin's (200) | soft target for tour length |
| `step_budget` | positive `int` | zin's (2,000,000) | hard limit on all emitted steps |
| `pair_limit` | positive `int` | none | sample size with `cover="pairs"` |
| `component` | `str` | whole system | test one block of a composition |
| `plan` | path | sibling `<spec>.plan.json` | an explicit plan file |
| `bin` | `str` (`load_plan` only) | `$ZIN_BIN`, then `zin` on `PATH` | the binary |

```python
@zin.verify("loan.zin", cover="states", max_steps=50)
class QuickLoan: ...

@zin.verify("loan.zin", plan="plans/loan-nightly.plan.json")
class NightlyLoan: ...
```

Relative paths in `verify` resolve against the file that defines the class;
in `check`, against the calling file. A committed sibling plan derived with
different options is skipped (and re-derived with the binary); an explicit one
is an error.

## Model coverage in pytest

The package registers a pytest plugin (entry point `pytest11`). With
`--zin-coverage` it prints, per suite, the distinct (model state, call)
operations the run attempted out of those planned and eligible, how many
failed, and which fields `observe` actually compared:

```console
$ pytest -q --zin-coverage test_loan.py
....
Zin test_loan.py::LoanConformance: operations 90/90 planned, 90 eligible, 0 failed; observed: holder, renewals, waiting
4 passed in 0.01s
```

Eligible is the model's transitions plus refusals (26 + 64 here). Name filters
(`-k`) and early failures lower the attempted count; replays that shorten a
failure are not counted. With `cover="pairs"` it also counts adjacent pairs.
From a source checkout that is not installed, load the plugin explicitly:
`python -m pytest -p zin.pytest_plugin --zin-coverage`.

## Without a test framework

```python
summary = zin.check("loan.zin", lambda: make_adapter(), cover="states")
```

`check(spec, factory, *, plan=None, **options)` runs every plan test and raises
`zin.Divergence` on the first failure; it returns the plan's `summary` dict.
The factory returns an adapter object (`setup` is not called for you here).

For finer control:

```python
plan = zin.load_plan("loan.zin", cover="pairs", pair_limit=50)
for test in plan["tests"]:
    zin.run_test(plan, test, make_adapter)
```

| Function | Signature |
| --- | --- |
| `load_plan` | `load_plan(spec, *, cover=None, plan=None, bin=None, max_steps=None, step_budget=None, pair_limit=None, component=None) -> dict` |
| `run_test` | `run_test(plan, test, factory) -> None`; raises `Divergence` |
| `check` | `check(spec, factory, *, plan=None, **options) -> dict` |
| `verify` | `verify(spec, *, plan=None, **options) -> Callable[[type], type]` |
| `fingerprint` | `fingerprint(source: bytes) -> str`, the FNV-1a 64 that plans record |

| Exception | Raised when |
| --- | --- |
| `zin.Rejected(Exception)` | you raise it: the implementation refused |
| `zin.Divergence(AssertionError)` | the implementation disagrees; `.fingerprint`, `.scenario` (pasteable) |
| `zin.PlanError(RuntimeError)` | the plan cannot be loaded, derived or matched to the adapter |

## Recording real executions

`zin.Recorder` writes the JSON-lines log that [`zin trace`](../conformance.md#checking-recorded-executions)
checks, from production code, a staging run or an integration test:

```python
with zin.Recorder("events.jsonl") as log:
    log.event("borrow", {"m": "ana"}, state={"holder": "ana", "renewals": 0, "waiting": set()})
    log.event("renew", accepted=False)
    log.reset()        # a new run starts from the initial state
```

| Member | Meaning |
| --- | --- |
| `Recorder(target)` | a path to append to, or any object with `write` (not closed by the recorder) |
| `event(action, args=None, *, accepted=True, state=None)` | `args` by name (`dict`) or positional (iterable); `state` any subset of fields, normalized like `observe` |
| `reset()` | writes `{"reset":true}` |
| `close()`, `with` | closes a file the recorder opened |

Record where the call takes effect, in the order calls took effect:

```console
$ zin trace loan.zin events.jsonl
violation: trace line 5: the implementation accepted borrow(ana), but the specification rejects it in state holder=nothing, renewals=0, waiting=[ben]
```

## A complete example

The running example of this section: a library book with an optional holder,
up to two renewals, and a reservation list.

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

The implementation, `library.py`:

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
        if self.waiting or self.renewals >= 2:
            raise ValueError("cannot renew")
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

The test, `test_loan.py`:

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

Four tests, 116 steps (52 accepted, 64 rejected). Drop `self.waiting or` from `renew` and the scenario
fails at once:

```text
zin.Divergence: Library loan: implementation diverges from the specification in test "scenario: A reservation blocks renewal" at step 4
the implementation accepted renew, but the specification rejects it
steps taken:
  1. borrow(ana)
  2. renew
  3. reserve(ben)
  4. reject renew
spec fingerprint: 09d83aa6c856ea87
scenario "Replay divergence"
  borrow(ana)
  renew
  reserve(ben)
  reject renew
```

## Troubleshooting

| Message | Cause |
| --- | --- |
| `LoanConformance has no method for the action 'give_back'` | a method is missing or misspelled (keywords need a trailing `_`) |
| `observe() must return a dict of state fields, not list` | `observe` returned something other than a `dict` or `None` |
| `cannot read specification loan.zin` | the path does not resolve against the test file's directory |
| `... the zin binary was not found. Install zin-spec, set ZIN_BIN, or regenerate the plan: zin plan ...` | no usable plan and no binary on `PATH` |
| `loan.plan.json was derived from a different version of loan.zin; regenerate it with: ...` | an explicit plan is stale |
| `... was derived with different planning options; regenerate it with: ...` | an explicit plan was derived with other options |
| `zin plan loan.zin failed:` followed by the checker's output | the specification does not check: fix it first |
| `planning options need a .zin specification, not a plan file` | options were passed with a `.plan.json` path |
| `max_steps must be a positive integer` | a non-positive or non-`int` option |
