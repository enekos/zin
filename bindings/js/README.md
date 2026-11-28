# zin-verify (JavaScript / TypeScript)

Run a [Zin](https://github.com/enekos/zin) specification as conformance tests
in Jest, Vitest, `node:test`, Mocha or Playwright. No runtime dependencies; CommonJS and ESM; types included.

```js
import { describe, test } from "vitest"; // or node:test; omit for Jest/Mocha globals
import { verify } from "zin-verify";

verify(new URL("./account.zin", import.meta.url), () => {
  const account = new Account();
  return {
    actions: { deposit: ({ amount }) => account.deposit(amount) },
    rejects: (error) => error instanceof RangeError,
    observe: () => ({ balance: account.balance }),
  };
}, { describe, test });
```

The plan comes from the `zin` binary (`ZIN_BIN` or `PATH`) or from a committed
`account.plan.json`. See
[conformance testing](https://github.com/enekos/zin/blob/main/docs/conformance.md).

API: `verify`, `check`, `loadPlan`, `runTest`, `Recorder`, `Rejected`,
`Divergence`, `PlanError`. `zin-verify/recorder` exports the `Recorder` alone,
with no Node.js dependency, for recording in a browser.

`observe()` may leave state fields out, but a field it returns must be
complete: a map lists every key and a record every member, at every depth. A
`Set` matches a set field in any order, never a list field. Await values before
returning them; a promise fails the test.

For an implementation that reaches its new state after the action returns,
give the adapter a `settle()` and pass `{ observeTimeout: 2000 }`; see
[implementations that settle later](https://github.com/enekos/zin/blob/main/docs/conformance.md#implementations-that-settle-later).

## Playwright

`zin-verify/playwright` runs the plan against a page: one Playwright test per
scenario and tour, one `test.step` per step.

```js
import { test } from "@playwright/test";
import { verify, click, gate } from "zin-verify/playwright";

verify(new URL("./editor.zin", import.meta.url), async ({ page }) => {
  const server = await gate(page, "**/api/quantity");
  await page.goto("/");
  return {
    actions: {
      save: () => server.request(() => click(page.getByRole("button", { name: "Save" }))),
      server_ok: () => server.fulfill({ json: { ok: true } }),
    },
    observe: async () => ({ status: await page.getByRole("status").textContent() }),
    teardown: () => server.dispose(),
  };
}, { test });
```

`click` and `fill` refuse an action a user could not perform (missing, hidden
or disabled); `gate` holds requests so that the server's answer is a step of
the plan. `@playwright/test` is an optional peer dependency. See
[checking a web interface](https://github.com/enekos/zin/blob/main/docs/browser.md).

## Jest

Install from npm:

```sh
npm install --save-dev zin-verify jest
```

For a CommonJS Jest project, create `account.test.cjs`:

```js
const { join } = require("node:path");
const { verify } = require("zin-verify");
const { Account } = require("./account.cjs");

verify(join(__dirname, "account.zin"), () => {
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

Run `npx jest`. Like the Python decorator, `verify` registers one native test
per derived scenario or tour, with a fresh adapter for each test. Call it at
file scope, outside `test` and hooks, so Jest can collect the cases. The factory,
actions, observation and optional `teardown` can be async. Cleanup runs even on
failure, including adapters created for a shorter reproduction.

String paths are relative to the working directory; use `join(__dirname, ...)`
for paths relative to a CommonJS test file. In ESM, use
`new URL("./account.zin", import.meta.url)`.

For ESM tests, import `verify` from `zin-verify` and `describe, test` from
`@jest/globals`, then pass `{ describe, test }` as the third argument. This also
works with Jest's `injectGlobals: false`. Follow
[Jest's ESM setup](https://jestjs.io/docs/ecmascript-modules): disable transforms
with `transform: {}` (or use an ESM-emitting transformer), and run:

```sh
node --experimental-vm-modules node_modules/jest/bin/jest.js
```

To run this binding's own checks, set `ZIN_BIN` to your built `zin` executable,
then run `npm ci && npm test`. CI runs both Jest module formats and `node:test`,
including a deliberately broken implementation whose failing Jest exit status
and per-case diagnostics are checked.

## Planning options and committed plans

`verify`, `check` and `loadPlan` take `cover`, `maxSteps`, `stepBudget`,
`pairLimit` and `component`, the options of `zin plan`. A committed sibling
`account.plan.json` (or an explicit `plan`) is used when it is fresh for the
specification and every included file, and was derived with each option you
pass. A sibling must also have been derived with zin's default for every
option you leave out, so a plan with weaker coverage is never picked up
silently; an explicit plan is your opt-in to its options. A path to a `.json` plan replays
that plan, after checking it against `account.zin` beside it. See
[the runner contract](../../docs/plan-format.md#what-a-binding-must-do).

A `Divergence` carries `fingerprint` and `scenario`: paste the scenario into
the specification to keep the failing prefix as a worked example.

## Vitest model coverage

Register the suite with `import { verify } from "zin-verify/vitest"` and add
the reporter (Vitest 3 or later, an optional peer dependency):

```js
import { defineConfig } from "vitest/config";
import ZinReporter from "zin-verify/vitest-reporter";

export default defineConfig({ test: { reporters: ["default", new ZinReporter()] } });
```

The report counts the unique (model state, call) operations attempted out of
those planned and eligible, the operations that failed, and the fields
actually compared. Name filters, failures and workers are accounted for;
replays that shorten a failure are not counted. With `cover: "pairs"` it also
counts adjacent calls.
