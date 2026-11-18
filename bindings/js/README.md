# zin-verify (JavaScript / TypeScript)

Run a [Zin](https://github.com/enekos/zin) specification as conformance tests
in Jest, Vitest, `node:test` or Mocha. No dependencies; ESM; types included.

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
