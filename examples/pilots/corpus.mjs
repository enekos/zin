import assert from "node:assert/strict";
import BrokenLRU from "quick-lru-broken";
import FixedLRU from "quick-lru-fixed";
import { loadPlan } from "zin-verify";
import { measure } from "./measure.mjs";

function acknowledgment(broken) {
  let count = 0;
  return {
    actions: { ack() { count++; return !broken || count === 1; } },
    observe: () => ({ acknowledged: true }),
  };
}

function account(mutation) {
  let balance = 0;
  return {
    actions: {
      credit() {
        if (balance === (mutation === "limit" ? 3 : 2)) return false;
        balance++;
      },
      debit() {
        if (balance === 0) {
          if (mutation === "refusal") balance--;
          return false;
        }
        balance--;
      },
    },
    observe: () => ({ balance }),
  };
}

function cache(Cache) {
  const lru = new Cache({ maxSize: 10, maxAge: 1 });
  let now = 1000;
  // The library reads Date.now synchronously. Restore it before returning to the runner.
  function atCurrentTick(operation) {
    const realNow = Date.now;
    Date.now = () => now;
    try { return operation(); } finally { Date.now = realNow; }
  }
  return {
    actions: {
      put() { atCurrentTick(() => lru.set("key", "value")); },
      tick() { now++; },
    },
    observe: () => ({ present: atCurrentTick(() => lru.has("key")) }),
  };
}

const cases = [
  { name: "second acknowledgement", spec: "ack", factory: broken => acknowledgment(broken) },
  { name: "credit limit", spec: "capacity", factory: broken => account(broken ? "limit" : undefined) },
  { name: "mutation on refusal", spec: "capacity", factory: broken => account(broken ? "refusal" : undefined) },
  { name: "quick-lru expiry on overwrite", spec: "expiry", factory: broken => cache(broken ? BrokenLRU : FixedLRU) },
];

const modes = process.argv.length > 2 ? process.argv.slice(2) : ["transitions", "pairs"];
for (const cover of modes) {
  assert.ok(["transitions", "pairs"].includes(cover), "choose transitions or pairs");
  for (const pilot of cases) {
    const plan = loadPlan(new URL(pilot.spec + ".zin", import.meta.url), { cover });
    for (const broken of [false, true]) {
      const row = await measure(plan, plan.tests, () => pilot.factory(broken));
      console.log(JSON.stringify({ pilot: pilot.name, variant: broken ? "faulty" : "fixed", ...row }));
      if (!broken) {
        assert.deepEqual(row.mismatches, [], pilot.name);
      } else if (cover === "transitions" && pilot.spec === "ack") {
        assert.deepEqual(row.mismatches, [], "the single model edge is exercised once");
      } else {
        assert.ok(row.mismatches.length > 0, "expected a divergence for " + pilot.name + " under " + cover);
      }
    }
  }
}
