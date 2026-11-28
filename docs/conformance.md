# Conformance testing

`zin check` proves things about the model. Conformance testing connects the
model to your code: the implementation must accept exactly the calls the
specification accepts, refuse the ones it refuses, and (where you expose it)
end up in the same state.

## How it works

1. `zin plan spec.zin` verifies the specification and derives a **plan**: a
   set of call sequences, each starting from the initial state. Scenarios come
   first, then generated *tours*.
2. A binding for your test framework loads the plan and registers one test per
   sequence.
3. Each test builds a fresh implementation through your **adapter** and
   replays the sequence. After every step it compares:
   - **accept / reject**: did the implementation allow the call?
   - **state**: whatever your optional `observe` returns, against the model.
     A rejected call must leave the state unchanged.
4. On a mismatch the test fails with the steps taken and, when a shorter path
   to the same model state shows the same failure, that shortest reproduction.

With the default coverage the plan takes **every transition of the model**
(every enabled call in every reachable state) and tries **every rejected call
in every reachable state**. For a deterministic implementation whose
observable state matches the model, that is the whole behavior of the model.

## The adapter

An adapter answers three questions.

**How do I perform each action?** One function per action; arguments arrive by
parameter name. Integers and booleans arrive as such, enum values as their
name in the specification (a string).

**How does the implementation say no?** A rejection is a normal outcome, not a
failure. Anything else that goes wrong (an unexpected exception, an error that
is not a rejection) fails the test as an error; it is never silently counted
as a rejection.

| Binding | An action is rejected when it |
| --- | --- |
| Python | raises `zin.Rejected` or a type listed in `rejects`, or returns `False` |
| JavaScript | throws `Rejected` or an error for which `rejects(error)` is true, or returns `false` |
| Go | returns an error wrapping `zin.ErrRejected` |
| Rust | returns `Outcome::Rejected`; `bool` and `Result<_, Refused>` convert, other errors go through `Outcome::refused_if` |

**What state can be seen?** `observe` is optional and may return any subset of
the specification's fields; only the returned fields are compared. Maps are
dictionaries / objects / Go maps keyed by the specification's key names, and
list every key: a field is either left out or reported in full. Without `observe`, wrong state is still detected once it
changes which calls are accepted, only later and with a less direct message.
How much later, and whether at all, is what [`zin observe`](#what-observe-can-tell-apart)
answers.

Mapping is the adapter's job. If the implementation stores cents and the
specification counts whole units, or uses its own enum type, convert in
`observe`. Python `enum.Enum` members and Go `fmt.Stringer` values are
converted for you.

### Python (pytest, unittest)

```python
import zin

@zin.verify("account.zin")           # path relative to this file
class AccountConformance:
    rejects = (ValueError,)

    def setup(self): ...             # before each test (optional)
    def teardown(self): ...          # after each test (optional)
    def deposit(self, amount): ...
    def observe(self): return {"balance": ...}
```

The decorator returns a `unittest.TestCase` subclass with one `test_NNN_...`
method per plan test, so pytest (`-k`, `-x`, xdist, reports) and plain
`python -m unittest` treat them as ordinary tests. For other runners,
`zin.check(spec, factory)` runs everything and raises `zin.Divergence`.

### JavaScript and TypeScript (Jest, Vitest, node:test, Mocha)

```js
import { verify } from "zin-verify";

verify(new URL("./account.zin", import.meta.url), async () => {
  const account = await Account.create();
  return {
    actions: { deposit: ({ amount }) => account.deposit(amount) },
    rejects: (error) => error instanceof RangeError,
    observe: () => ({ balance: account.balance }),
    teardown: () => account.dispose(),
  };
});
```

`verify` uses the global `describe` / `test` (Jest, Vitest with globals,
Mocha). Otherwise pass them: `verify(spec, factory, { describe, test })` with
the imports from `vitest` or `node:test`. The factory, actions, `observe` and
`teardown` may all be async. `check(spec, factory)` is the framework-free form.

For CommonJS Jest projects, use `const { verify } = require("zin-verify")`
and `path.join(__dirname, "account.zin")`. Call `verify` at file scope: each
scenario and tour is collected as its own native Jest test, with a fresh
adapter and awaited cleanup. ESM Jest projects need Jest's VM-module setup;
see the [complete Jest setup and example](https://github.com/enekos/zin/blob/main/bindings/js/README.md#jest).

For a page in a browser, `zin-verify/playwright` registers the plan with
Playwright Test and adds helpers for clicks and held requests; see
[checking a web interface](browser.md).

#### Implementations that settle later

A synchronous implementation is in its new state when the action returns. One
that is not (a rendered interface, a queue, anything behind a network) gets
two tools, both off by default:

- `settle` on the adapter is awaited after every action, before the state is
  observed. Wait there for whatever "at rest" means: no request in flight, a
  job drained.
- `observeTimeout` (milliseconds) keeps calling `observe` until the state
  matches the specification, and fails with the last observation when it never
  does: `verify(spec, factory, { observeTimeout: 2000 })`.

`observeTimeout` cannot notice a state that changes *after* it matched, so a
rejected call that takes effect late needs `settle`. `maxSteps` passes
`--max-steps` to `zin plan`, and `step: (text, body) => body()` wraps every
step for frameworks that report steps.

### Go

```go
type adapter struct{ account Account }

func (a *adapter) Call(action string, args zin.Args) error {
	switch action {
	case "deposit":
		if err := a.account.Deposit(args.Int("amount")); err != nil {
			return fmt.Errorf("%w: %v", zin.ErrRejected, err)
		}
		return nil
	}
	return fmt.Errorf("unknown action %s", action)
}

func (a *adapter) Observe() map[string]any { // optional
	return map[string]any{"balance": a.account.Balance}
}

func TestAccount(t *testing.T) {
	zin.Verify(t, "account.zin", func() zin.Adapter { return &adapter{} })
}
```

Each plan test is a subtest (`go test -run 'TestAccount/tour_1'`).
`zin.Check` returns the failure as an error instead.

### Rust

```rust
use zin::conform::{Adapter, Args, Outcome, assert_conforms};

impl Adapter for Harness {
    fn call(&mut self, action: &str, args: &Args) -> Outcome {
        match action {
            // Only a refusal is a rejection; any other error panics and fails the test.
            "close" => Outcome::refused_if(self.account.close(), |error| {
                matches!(error, AccountError::NotEmpty | AccountError::Closed)
            }),
            other => panic!("unknown action {other}"),
        }
    }
}

#[test]
fn conforms() {
    assert_conforms(include_str!("../account.zin"), Harness::default);
}
```

The Rust binding is the `zin` crate itself (a dev-dependency); the plan is
derived in-process, so there is nothing to install or commit. A specification
that uses `include` needs its files, not just its text: call
`assert_conforms_file(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/spec.zin"), ...)`
(or `check_file`), which is what `zin init rust` generates.

### JUnit 5

```java
import io.github.enekos.zin.Zin;

class AccountTest {
    static class Harness implements Zin.Adapter {
        final Account account = new Account();

        @Override
        public boolean call(String action, Map<String, Object> args) {
            switch (action) {
                case "deposit" -> account.deposit((Long) args.get("amount"));
                case "close" -> { if (account.balance() != 0) return false; account.close(); }
                default -> throw new IllegalArgumentException(action);
            }
            return true;
        }

        @Override
        public Map<String, ?> observe() { // optional
            return Map.of("balance", account.balance());
        }
    }

    @TestFactory
    Stream<DynamicTest> conforms() throws IOException {
        return Zin.load(Zin.resource(AccountTest.class, "account.zin")).tests(Harness::new);
    }
}
```

Each plan test is a dynamic test. Put `account.zin`, and a committed
`account.plan.json` if you want one, in `src/test/resources`. Arguments arrive
by parameter name as `Long`, `Boolean` or `String`. Return `false` for a
refusal; any exception fails the test. `zin init junit account.zin` prints
this skeleton. See [the binding's README](https://github.com/enekos/zin/blob/main/bindings/junit/README.md).

## Where the plan comes from

The Python, JavaScript, Go and JUnit bindings look, in order, for:

1. a plan file you pass explicitly (`plan=`, `{ plan }`, `zin.WithPlan`,
   `Options.plan`);
2. `<spec>.plan.json` next to the specification;
3. the `zin` binary (`ZIN_BIN`, then `PATH`), which they run as
   `zin plan <spec>` with the planning options you passed.

A plan file is used only if it is fresh for the specification and every file
it includes, and was derived with each planning option you passed (`cover`,
`max_steps`, `step_budget`, `pair_limit`, `component`). A sibling plan must
also use zin's default for every option you leave out, so a committed plan
with weaker coverage (say `--cover scenarios`) is never picked up silently;
naming a plan explicitly is the opt-in to its options. All runners follow the same
[contract](plan-format.md#what-a-binding-must-do) and run the same cases.

So you choose per project:

- **Derive at test time.** Install `zin` on developer machines and CI. The
  specification is re-verified on every test run, and a broken specification
  fails the tests with the checker's counterexample.
- **Commit the plan.** Run `zin plan spec.zin --out spec.plan.json` and commit
  it. Test machines need nothing but the binding. The plan records a
  fingerprint of the specification; if the specification changes and the plan
  is not regenerated, loading fails with the command to run. Never a stale
  pass.

## Coverage

`--cover` (or `cover=`) trades thoroughness for time:

| Cover | The plan contains |
| --- | --- |
| `transitions` (default) | every transition, and every rejected call in every state |
| `states` | a visit to every reachable state |
| `actions` | every action accepted once and, where possible, rejected once |
| `scenarios` | only the scenarios you wrote |
| `pairs` | every pair of adjacent calls from every reachable state, including repeated and rejected calls |

`transitions` checks each call once per model state. An implementation that
keeps history the model does not, such as an `ack` that accepts the first
acknowledgement and rejects the second, passes it: the model is in the same
state both times. `pairs` makes each (state, first call, second call) an
obligation. A refusal leaves the model in place, so `ack, ack` and
`reject close, deposit(1)` count. Pairs are chained into tours like
transitions, so a pair plan costs roughly one step per pair, not a fresh
setup per pair. `--pair-limit N` selects a deterministic sample, repetitions
first. The summary reports covered and eligible pairs; covered counts every
distinct adjacent pair the plan exercises, setup included.

`--max-steps` (default 200) is a soft target for tour length. It does not
count the shortest setup to the state a tour has reached, so a deep model is
covered in one long tour instead of restarting from the initial state for
every few calls. A tour ends early only when restarting is cheaper than
walking on.

`--step-budget` is a hard ceiling on every emitted step: authored scenarios,
`sometimes` witnesses, routes between obligations and refusals (default and
maximum 2,000,000). A plan that cannot fit exits with a limit error instead
of silently covering less. `scenarios` cover runs only authored scenarios, so
it skips generated witnesses too.

The number of steps is roughly `states × calls`. When that is too many for a
slow implementation (a database, a browser), shrink the model's ranges for
testing, use `states`, or run `transitions` nightly.

## What observe can tell apart

A runner sees whether each call is accepted and the fields `observe` returns.
Two states with the same observation are told apart only by a later call that
one accepts and the other rejects, or that leads them to different
observations. `zin observe` computes this for a given set of fields:

```console
$ zin observe examples/editor.zin --fields draft,status
ok Quantity editor: 36 states, 96 transitions, 5 properties, 4 scenarios
observing draft, status: 36 states show 9 distinct observations
states that look alike are told apart by at most 3 further calls, for example
  draft=0, saved=0, sent=1, status=saving
  draft=0, saved=0, sent=2, status=saving
  by: increase, server_ok, reject save
default plan: 53 of 96 transitions could reach a look-alike state unnoticed; zin plan --observe draft,status adds 48 tests (427 steps)
every state can be told apart
```

Without `--fields` it answers for an adapter with no `observe` at all.

- **Blind states.** When no sequence of calls separates two states, no test
  can notice the implementation being in one instead of the other. The
  command lists them with the fields they differ in and exits `1`: observe
  one of those fields, or the specification carries state nothing depends on.
- **Look-alike states.** The default plan takes every transition once, but
  what follows it in the tour may not separate the state reached from one
  that looks the same. An implementation whose `save` stores the wrong `sent`
  then passes. `zin plan --observe draft,status` adds `identify` tests: the
  shortest setup, the transition, then the shortest calls that every
  look-alike state would answer differently. `--observe ''` plans for an
  adapter without `observe`.

The identify tests assume the adapter returns exactly the named fields.
Commit the plan next to the specification (`--out`) to have every runner use
it; a runner does not yet check that the adapter observes those fields. The
guarantee is the one of state identification in finite-state testing: a
single transition that lands in another *specified* state is caught. An
implementation with states the specification does not have is outside it.
The extra tests count against `--step-budget`; a model with too many
look-alike states exits with a limit error that says to observe more fields.

## What it does and does not show

- It is exhaustive over the **model**, not over the implementation's code. An
  implementation that behaves differently for `amount = 6` when the model only
  goes to 5 is not exercised there.
- Each test drives the implementation **sequentially**. Concurrency is
  explored in the model (where interleavings are exhaustive); conformance then
  checks that each atomic step of the implementation matches. To check real
  concurrent runs, record them and use `zin trace`.
- State the specification does not model is invisible, except that the report
  tells you when a failure depends on it ("the shortest path to the same
  specification state does not reproduce the original mismatch"). A shorter
  reproduction is only reported when it fails at the same call with the same
  mismatch.
- Liveness is proven about the model. A test cannot show that an
  implementation eventually does something.

## Checking recorded executions

`zin trace spec.zin events.jsonl` replays a log against the model. One JSON
object per line:

```json
{"action":"deposit","args":{"amount":5},"outcome":"accepted","state":{"balance":5}}
{"action":"close","outcome":"rejected"}
{"reset":true}
```

- `args`: by name, or positional as an array; may be omitted for none.
- `outcome`: `accepted` (default) or `rejected`.
- `state`: optional, any subset of fields, compared after the event.
- `{"reset":true}`: a new run starts from the initial state.

The log must be a **linearized** history of one instance of the system: the
order in the file is the order in which the calls took effect. `Recorder` in
the Python and JavaScript bindings writes this format; `zin-verify/recorder`
is the same recorder for code that runs in a browser.

## Auditing the specification

Conformance is only as strong as the specification. `zin audit spec.zin`
applies small changes (drop a `requires`, drop an assignment, flip a
comparison, shift a constant) and re-checks each mutant:

- **caught**: a property or scenario fails. Good.
- **SURVIVED**: the reachable behavior changed and nothing noticed. The report
  shows a shortest distinguishing prefix, the call, and both outcomes. Add a
  property or a scenario (a `reject` line is often enough), or delete the
  redundant rule.
- **same**: the reachable behavior is unchanged; the rule is implied by others.

`zin audit` exits 1 when a mutant survives, so it can gate CI.

## CI

```yaml
- run: cargo install zin-spec
- run: zin check specs/*.zin
- run: zin audit specs/account.zin
- run: pytest            # or npm test, go test ./..., cargo test
```

`zin check --format json` prints one machine-readable result per file,
including counterexample traces.
