---
title: JavaScript and TypeScript
parent: Bindings
nav_order: 2
---

# JavaScript and TypeScript
{: .no_toc }

`zin-verify` registers a specification's plan as native tests in Jest, Vitest,
`node:test` and Mocha. No runtime dependencies, CommonJS and ESM, TypeScript
types included. Node.js 18+.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

```sh
npm install --save-dev zin-verify
cargo install zin-spec        # the zin binary, unless you commit plans
```

| Entry point | Exports |
| --- | --- |
| `zin-verify` | `verify`, `check`, `loadPlan`, `runTest`, `fingerprint`, `Recorder`, `Rejected`, `Divergence`, `PlanError`, `PLAN_FORMAT` |
| `zin-verify/vitest` | `verify` that also records model coverage (ESM only) |
| `zin-verify/vitest-reporter` | the `ZinReporter` default export (ESM only) |
| `zin-verify/playwright` | see [Playwright and components](playwright.md) |
| `zin-verify/recorder` | `Recorder` with no Node.js dependency, for browsers |

## Minimal example

```js
import { describe, test } from "node:test"; // or vitest; omit for Jest and Mocha globals
import { verify } from "zin-verify";
import { Account } from "./account.js";

verify(new URL("./account.zin", import.meta.url), () => {
  const account = new Account();
  return {
    actions: {
      deposit: ({ amount }) => account.deposit(amount),
      withdraw: ({ amount }) => account.withdraw(amount),
    },
    rejects: (error) => error instanceof RangeError,
    observe: () => ({ balance: account.balance }),
  };
}, { describe, test });
```

`zin init jest|vitest|node account.zin` prints this for your specification.

## The factory and the adapter

`verify(spec, factory, options)` calls `factory()` once per plan test (and once
more to replay a shorter reproduction after a failure). The factory may be
async and returns an adapter:

| Key | Type | Required | Meaning |
| --- | --- | --- | --- |
| `actions` | `Record<string, (args) => unknown>` | yes | one function per action; `args` is an object keyed by parameter name |
| `rejects` | `(error: unknown) => boolean` | no | which thrown errors mean "refused" |
| `observe` | `() => object \| null \| undefined` | no | any subset of the state fields; `null`/`undefined` compares nothing |
| `settle` | `() => void` | no | awaited after every action, before observing |
| `teardown` | `() => void` | no | awaited after each run, also when it fails |

Every function may return a promise. Action functions are called with `this`
bound to the `actions` object.

### How arguments arrive

`deposit(amount is an integer from 1 to 5)` is called as
`actions.deposit({ amount: 3 })`. Integers arrive as `number`, booleans as
`boolean`, enum values as their name (`string`). An action without parameters
receives `{}`.

### How the implementation says no

A call is **rejected** when the action function:

- throws `Rejected` (`import { Rejected } from "zin-verify"`), or
- throws an error for which `rejects(error)` returns `true`, or
- returns (or resolves to) exactly `false`.

Anything else thrown propagates and fails the test with its own stack. It is
never counted as a rejection, so keep `rejects` specific.

```js
actions: {
  close: () => {
    if (account.balance !== 0) throw new Rejected("not empty");
    account.close();
  },
  reopen: () => account.reopen(),            // returns false when refused
},
```

### What observe may return

An object keyed by state field names; only those fields are compared, and a
name that is not a field fails the test. Values:

| JavaScript | Compared as |
| --- | --- |
| `number`, `boolean`, `string` | themselves, strictly (`false` is not `0`) |
| `null`, `undefined` | nothing |
| plain object | a map (keys as text) or a record; lists every key |
| `Map` | a map, keys converted with `String(key)` |
| `Set` | a set, in any order |
| array | a list, in order |
| `{ running: { attempt: 2 } }` | an enumeration case with members |

Observe a set field as a `Set`; an array is compared in the order the element
type declares its values.

## Options

`verify(spec, factory, options)` takes planning options, run options and
registration options. `spec` is a path string (relative to the working
directory), a `file:` URL string, or a `URL`.

### Planning options (`PlanOptions`)

| Option | Type | Default | `zin plan` flag |
| --- | --- | --- | --- |
| `cover` | `"transitions" \| "states" \| "actions" \| "scenarios" \| "pairs"` | zin's (`transitions`) | `--cover` |
| `maxSteps` | positive integer | zin's (200) | `--max-steps` |
| `stepBudget` | positive integer | zin's (2,000,000) | `--step-budget` |
| `pairLimit` | positive integer | none | `--pair-limit` |
| `component` | string | whole system | `--component` |
| `plan` | `string \| URL` | sibling `<spec>.plan.json` | an explicit plan file |
| `bin` | string | `$ZIN_BIN`, then `zin` on `PATH` | the binary |

### Run options (`RunOptions`)

| Option | Type | Default | Meaning |
| --- | --- | --- | --- |
| `observeTimeout` | ms | `0` | keep observing until the state matches, up to this long |
| `observeInterval` | ms | `20` | pause between two observations |
| `step` | `(text, body) => body()` | none | wraps every step, for frameworks that report steps |

### Registration options (`VerifyOptions`)

| Option | Default | Meaning |
| --- | --- | --- |
| `describe` | `globalThis.describe` | the suite function |
| `test` / `it` | `globalThis.test`, then `globalThis.it` | the test function |
| `name` | `"<system> conforms to <file>"` | the suite name |

Without `describe` and `test` (passed or global), `verify` throws
`no test framework globals found; pass them in: verify(spec, factory, { describe, test })`.

## Implementations that settle later

A synchronous implementation is in its new state when the action returns. One
behind a queue, a timer or a network is not. Two tools, both off by default:

- **`settle`** on the adapter is awaited after every action, before observing.
  Wait there for "at rest": no request in flight, a queue drained.
- **`observeTimeout`** keeps calling `observe` until the observed values match
  the model, then fails with the last observation and `(waited N ms)`.

```js
verify(spec, async () => {
  const worker = await Worker.start();
  return {
    actions: { enqueue: ({ job }) => worker.enqueue(job) },
    settle: () => worker.idle(),
    observe: () => ({ queued: worker.size }),
    teardown: () => worker.stop(),
  };
}, { describe, test, observeTimeout: 2000 });
```

`observeTimeout` cannot notice a state that changes *after* it matched, so a
rejected call that takes effect late still needs `settle`. Only a wrong value
waits: an unknown field or a non-object fails at once.

## Per framework

### Jest, CommonJS

```js
// account.test.cjs
const { join } = require("node:path");
const { verify } = require("zin-verify");
const { Account } = require("./account.cjs");

verify(join(__dirname, "account.zin"), () => {
  const account = new Account();
  return {
    actions: { deposit: ({ amount }) => account.deposit(amount) },
    rejects: (error) => error instanceof RangeError,
    observe: () => ({ balance: account.balance }),
  };
});
```

Call `verify` at file scope, outside `test` and hooks, so Jest collects each
plan test as its own test. String paths are relative to the working
directory: use `join(__dirname, ...)`.

### Jest, ESM

```js
// account.test.mjs
import { describe, test } from "@jest/globals";
import { verify } from "zin-verify";

verify(new URL("./account.zin", import.meta.url), factory, { describe, test });
```

Follow [Jest's ESM setup](https://jestjs.io/docs/ecmascript-modules): no
transform (`transform: {}`) or an ESM-emitting one, and run

```sh
node --experimental-vm-modules node_modules/jest/bin/jest.js
```

Passing `{ describe, test }` also works with `injectGlobals: false`.

### Vitest

```js
import { describe, test } from "vitest";
import { verify } from "zin-verify";

verify(new URL("./account.zin", import.meta.url), factory, { describe, test });
```

With `globals: true` you may omit `{ describe, test }`. For model coverage use
`zin-verify/vitest` instead, which imports `describe` and `test` from Vitest
itself:

```js
import { verify } from "zin-verify/vitest";

verify(new URL("./account.zin", import.meta.url), factory);
```

and add the reporter (Vitest 3+):

```js
// vitest.config.js
import { defineConfig } from "vitest/config";
import ZinReporter from "zin-verify/vitest-reporter";

export default defineConfig({ test: { reporters: ["default", new ZinReporter()] } });
```

At the end of the run it prints one line per suite:

```text
Zin Library loan conforms to loan.zin: operations 90/90 planned, 90 eligible, 0 failed; observed: holder, renewals, waiting
```

Operations are distinct (model state, call) pairs attempted; eligible is the
model's transitions plus refusals. Name filters, failures and workers are
accounted for; replays that shorten a failure are not counted. With
`cover: "pairs"` it also reports adjacent pairs.

Under jsdom, pass the specification as a path, not `new URL(..., import.meta.url)`:
Vite resolves that URL to an `http:` address there.

### node:test

```js
import { describe, test } from "node:test";
import { verify } from "zin-verify";

verify(new URL("./account.zin", import.meta.url), factory, { describe, test });
```

```console
$ node --test
```

### Mocha

Mocha's BDD globals are `describe` and `it`; `verify` finds them:

```js
const { join } = require("node:path");
const { verify } = require("zin-verify");

verify(join(__dirname, "account.zin"), factory);
```

### Filtering one test

Plan test names are `scenario: <name>`, `reaches: <property>`, `tour <n>` and
`identify ...`:

| Framework | One test |
| --- | --- |
| Jest | `npx jest -t "tour 1"` |
| Vitest | `npx vitest run -t "tour 1"` |
| node:test | `node --test --test-name-pattern="tour 1"` |
| Mocha | `npx mocha --grep "tour 1"` |

## TypeScript

Types ship with the package (`index.d.ts`, `index.d.cts`). The useful ones:

```ts
import { verify, type Adapter, type Factory, type Observed } from "zin-verify";

const factory: Factory = () => {
  const account = new Account();
  const adapter: Adapter = {
    actions: { deposit: ({ amount }) => account.deposit(amount as number) },
    observe: (): Observed => ({ balance: account.balance }),
  };
  return adapter;
};
```

Arguments are typed `Record<string, Scalar>` where `Scalar` is
`number | boolean | string`; narrow them where you use them.

## Without a test framework

```js
import { check, loadPlan, runTest } from "zin-verify";

const summary = await check("loan.zin", factory, { cover: "states" }); // throws Divergence

const plan = loadPlan("loan.zin", { cover: "pairs", pairLimit: 50 });
for (const test of plan.tests) await runTest(plan, test, factory, { observeTimeout: 500 });
```

| Function | Signature |
| --- | --- |
| `verify` | `(spec, factory, options?: VerifyOptions) => Plan`; registers tests, returns the plan |
| `check` | `(spec, factory, options?: PlanOptions & RunOptions) => Promise<summary>` |
| `loadPlan` | `(spec, options?: PlanOptions) => Plan`; synchronous |
| `runTest` | `(plan, test, factory, options?: RunOptions) => Promise<void>` |
| `fingerprint` | `(bytes: Uint8Array) => string`, the FNV-1a 64 a plan records |

| Error | When |
| --- | --- |
| `Rejected` | you throw it: the implementation refused |
| `Divergence` | the implementation disagrees; `.fingerprint`, `.scenario` (pasteable) |
| `PlanError` | the plan cannot be loaded, derived, or matched to the adapter |

## Recording real executions

`Recorder` writes the log [`zin trace`](../conformance.md#checking-recorded-executions)
checks.

```js
import { Recorder } from "zin-verify";

const log = new Recorder("events.jsonl");     // a path or URL to append to
log.event("borrow", { m: "ana" }, { state: { holder: "ana", waiting: new Set() } });
log.event("renew", {}, { accepted: false });
log.reset();                                  // a new run from the initial state
```

| Member | Meaning |
| --- | --- |
| `new Recorder(target?)` | a path or `URL` to append to, a function receiving each line, or nothing to keep lines in `lines` |
| `event(action, args?, { accepted = true, state }?)` | `args` by name (object) or positional (array); `state` any subset of fields, normalized like `observe` |
| `reset()` | writes `{"reset":true}` |
| `lines`, `text()` | the kept lines, when no target was given |

`zin-verify/recorder` is the same class without Node.js: its constructor takes
only a write function, or nothing. Use it in a browser and ship
`session.text()` to wherever you run `zin trace`; see
[sessions nobody scripted](playwright.md#sessions-nobody-scripted).

## A complete example

The [Library loan](python.md#a-complete-example) specification against a
JavaScript class:

```js
// library.js
export class Book {
  holder = null;
  renewals = 0;
  waiting = [];

  borrow(member) {
    if (this.holder !== null) throw new RangeError("already on loan");
    if (this.waiting.length > 0 && !this.waiting.includes(member)) throw new RangeError("reserved for someone else");
    this.holder = member;
    this.waiting = this.waiting.filter((m) => m !== member);
  }

  renew() {
    if (this.holder === null || this.waiting.length > 0 || this.renewals >= 2) throw new RangeError("cannot renew");
    this.renewals += 1;
  }

  giveBack() {
    if (this.holder === null) throw new RangeError("not on loan");
    this.holder = null;
    this.renewals = 0;
  }

  reserve(member) {
    if (this.holder === null || this.holder === member || this.waiting.includes(member)) {
      throw new RangeError("cannot reserve");
    }
    this.waiting.push(member);
  }
}
```

```js
// loan.test.js
import { describe, test } from "node:test";
import { verify } from "zin-verify";
import { Book } from "./library.js";

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
$ node --test loan.test.js
▶ Library loan conforms to loan.zin
  ✔ scenario: A reservation blocks renewal (0.657084ms)
  ✔ scenario: Only the holder's book can come back (0.232875ms)
  ✔ reaches: sometimes renewals is 2 (0.098583ms)
  ✔ tour 1 (1.345583ms)
✔ Library loan conforms to loan.zin (2.832ms)
```

The action names come from the specification (`give_back`); the adapter maps
them onto your own names (`giveBack`).

## Troubleshooting

| Message | Cause |
| --- | --- |
| `the adapter has no function for the action 'give_back'` | `actions` lacks a key for that action |
| `no test framework globals found; pass them in: ...` | Vitest without globals, `node:test`, or ESM Jest: pass `{ describe, test }` |
| `observe() must return an object of state fields, not string` | `observe` returned a primitive |
| `observe() returned 'x', which is not a state field of the specification` | a misspelled or extra key |
| `cannot read specification ...` | the path is relative to the working directory, not the test file |
| `... the zin binary was not found. Install zin-spec, set ZIN_BIN, or regenerate the plan: ...` | no usable plan, no binary |
| `... was derived from a different version of ...; regenerate it with: ...` | an explicit or path-given plan is stale |
| `maxSteps must be a positive integer, not 0` | an invalid numeric option |
| `planning options need a .zin specification, not a plan file` | options passed with a `.json` path |
| `... (waited 2000 ms)` at the end of a mismatch | the state never matched within `observeTimeout` |
