# Zin

**Write the behavior once, in words. Prove it. Then let it test your code.**

**Zin** is Basque for a solemn promise. A Zin specification reads like a
behavioral contract, and the `zin` tool does three things with it:

1. **Proves it.** Every reachable state is explored. Safety, liveness and
   deadlock properties either hold for all of them or you get the shortest
   counterexample.
2. **Turns it into tests.** From the proven model, Zin derives a test plan that
   exercises every transition and every refusal, and runs it inside the test
   framework you already use: pytest, unittest, Jest, Vitest, `node:test`,
   Mocha, Playwright, `go test`, `cargo test`, JUnit 5.
3. **Keeps it honest.** `zin audit` mutates the specification to show which
   rules nothing checks; `zin trace` checks logs of real executions against it.

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

The specification above has 22 states and 182 transitions. You do not write 182
test cases. You write one adapter that says how each action maps onto your
code:

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

`pytest` now runs the derived plan: every action in every state where the
specification allows it, and every action in every state where it must be
refused. An implementation whose limit is 21 instead of 20 fails like this:

```text
Bank account: implementation diverges from the specification in test "tour 1" at step 63
the implementation accepted deposit(5), but the specification rejects it
steps taken:
  ... 55 earlier steps
  56. deposit(1)
  57. reject close
  58. reject reopen
  59. deposit(1)
  60. reject close
  61. reject reopen
  62. deposit(1)
  63. reject deposit(5)
shortest reproduction:
  1. deposit(1)
  2. deposit(5)
  3. deposit(5)
  4. deposit(5)
  5. reject deposit(5)
```

The same adapter idea in the other bindings:

```js
// Jest, Vitest, node:test, Mocha
import { verify } from "zin-verify";

verify(new URL("./account.zin", import.meta.url), () => {
  const account = new Account();
  return {
    actions: {
      deposit: ({ amount }) => account.deposit(amount),
      withdraw: ({ amount }) => account.withdraw(amount),
      close: () => account.close(),
      reopen: () => account.reopen(),
    },
    rejects: (error) => error instanceof RangeError,
    observe: () => ({ balance: account.balance, status: account.status }),
  };
});
```

```go
func TestAccount(t *testing.T) {
	zin.Verify(t, "account.zin", func() zin.Adapter { return &adapter{} })
}
```

```rust
#[test]
fn conforms_to_specification() {
    zin::conform::assert_conforms(include_str!("account.zin"), || Harness::default());
}
```

`zin init <framework> <spec>` prints the skeleton for your specification. See
[conformance testing](docs/conformance.md).

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

Liveness is checked under the fairness you declare (`fair action`,
`strongly fair action`), and a
violation is reported as a loop:

```console
$ zin check examples/broken/stuck-job.zin
violation: line 13: 'eventually status is succeeded' fails: the system can run forever without it happening
counterexample:
  start: status=queued
  then forever:
  1. start  ->  status=running
  2. crash  ->  status=queued
```

`succeed` is weakly fair, and a crash keeps disabling it. Declaring
`strongly fair action succeed` (a call enabled again and again is eventually
taken) makes the property hold.

Concurrency bugs are ordinary counterexamples. `examples/broken/lost-update.zin`
models two workers doing read-then-write on a shared counter:

```console
$ zin check examples/broken/lost-update.zin
violation: line 21: 'always (every w in worker satisfies at of w is done) implies counter is 2' is violated
counterexample:
  start: at={alice: idle, bob: idle}, counter=0, seen={alice: 0, bob: 0}
  1. read(alice)  ->  at={alice: holding, bob: idle}
  2. read(bob)  ->  at={alice: holding, bob: holding}
  3. write(alice)  ->  at={alice: done, bob: holding}, counter=1
  4. write(bob)  ->  at={alice: done, bob: done}
```

Exploration is exhaustive within the declared finite domains. Hitting the state
limit is an error, never a silent partial pass.

Larger systems split into components: several `system` blocks in one file
share no state and synchronize on actions with the same name
(`examples/request-reply.zin`). Identical components declared
`interchangeable` are explored once per arrangement up to renaming
(`examples/mutex.zin`). See [the language reference](docs/language.md).

## Is the specification itself any good?

A specification that allows everything proves nothing. `zin audit` removes and
alters each rule in turn and reports the changes that no property or scenario
notices:

```console
$ zin audit delivery.zin
caught   line 21: action pack: drop 'requires paid' ('always stage is packed implies paid' is violated)
SURVIVED line 27: action ship: drop 'requires stage is packed' (no property or scenario notices)
...
16 behavior-changing mutants: 15 caught, 1 survived (1 equivalent, 0 skipped)
```

Each survivor is a rule you can delete without anything failing: either it is
redundant or a property is missing.

`zin observe` asks the same of the adapter: given the fields `observe`
returns, which states can a test run never tell apart, and which transitions
could land in a look-alike state without the plan noticing?
`zin plan --observe a,b` adds the tests that close the second gap. See
[conformance](docs/conformance.md#what-observe-can-tell-apart).

## Checking real executions

Have a service, a staging run, or a flaky integration test? Log one JSON line
per call and check the log against the specification:

```console
$ zin trace account.zin events.jsonl
violation: trace line 6: the implementation accepted deposit(1), but the specification rejects it in state balance=0, status=closed
```

The bindings ship a `Recorder` that writes this format.

## Commands

```text
zin check <spec>... [--format json] [--symmetry on|off]
                                      prove every property over every reachable state
zin plan <spec> [--out FILE] [--cover transitions|states|actions|scenarios|pairs] [--max-steps N] [--step-budget N] [--pair-limit N]
         [--observe a,b]              also confirm each state reached under that observation
zin observe <spec> [--fields a,b]     which states a test run can tell apart
zin trace <spec> <events.jsonl|->     check a recorded execution
zin audit <spec>                      list specification changes nothing notices
zin init <framework> <spec>           print a conformance test skeleton
zin report <spec>                     Markdown report with Mermaid diagrams
zin diagram <spec> [scenario]         Mermaid state or sequence diagram
```

Exit codes: `0` ok, `1` the specification or implementation is wrong, `2` the
input could not be used.

## Install

```sh
cargo install zin-spec            # the zin binary (Rust 1.88+); crate zin-spec, library `zin`
npm install --save-dev zin-verify # Jest, Vitest, node:test, Mocha, Playwright; CommonJS + ESM, types included
go get github.com/enekos/zin/bindings/go
pip install /path/to/zin/bindings/python  # pytest / unittest binding
```

`zin-verify` is not on PyPI yet; install the Python binding from a checkout
until it is. The crate and the npm package are published.

Prebuilt `zin` binaries for Linux and macOS (x86_64, arm64) are attached to
each [GitHub release](https://github.com/enekos/zin/releases).

The bindings have no dependencies. They call `zin plan` at test time, or read
a committed `<spec>.plan.json` so test machines need no Rust toolchain.

## Documentation

- [Language guide](docs/language.md): the complete grammar and semantics
- [Conformance testing](docs/conformance.md): adapters, coverage, CI, traces
- [Checking a web interface](docs/browser.md): Playwright, clicks, held requests
- [Plan format](docs/plan-format.md): the JSON contract for writing a binding
- [Design](docs/design.md): what is proven, what is not, and why it is built this way

## License

MIT — see [LICENSE](LICENSE).

## Scope

Zin models finite-state systems: bounded integers, booleans and enums, and
records, maps, optional values, sets, bounded lists and enumerations with
members built from them. It does not model unbounded data, real time, probabilities or
floating point. Conformance testing shows that an implementation agrees with
the model on every modeled transition; it is exhaustive over the model, not a
proof about the implementation's code.

## Develop

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cargo build && export ZIN_BIN=$PWD/target/debug/zin
(cd bindings/python && python -m pytest)
(cd bindings/js && node --test)
(cd bindings/go && go test ./...)
mvn -f bindings/junit/pom.xml test
```
