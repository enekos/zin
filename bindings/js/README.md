# zin-verify (JavaScript / TypeScript)

Run a [Zin](https://github.com/enekos/zin) specification as conformance tests
in Jest, Vitest, `node:test` or Mocha. No runtime dependencies; CommonJS and ESM; types included.

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
`Divergence`, `PlanError`.

For an implementation that reaches its new state after the action returns,
give the adapter a `settle()` and pass `{ observeTimeout: 2000 }`; see
[implementations that settle later](https://github.com/enekos/zin/blob/main/docs/conformance.md#implementations-that-settle-later).

## Jest

Install from a checkout (the package is not published on npm yet):

```sh
npm install /path/to/zin/bindings/js
npm install --save-dev jest
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
