import { performance } from "node:perf_hooks";
import { Divergence, runTest } from "zin-verify";

// Count actual calls (including reproduction attempts) and fields returned by observers.
export async function measure(plan, tests, factory, options = {}) {
  const mismatches = new Set();
  const fields = new Set();
  let calls = 0;
  let setups = 0;
  let firstFailureMs = null;
  const start = performance.now();
  async function instrumented() {
    setups++;
    const adapter = await factory();
    const actions = Object.fromEntries(Object.entries(adapter.actions).map(([name, action]) => [
      name,
      (...args) => { calls++; return action.apply(adapter.actions, args); },
    ]));
    return {
      ...adapter,
      actions,
      async observe() {
        const value = await adapter.observe?.();
        for (const field of Object.keys(value ?? {})) fields.add(field);
        return value ?? {};
      },
    };
  }
  for (const tour of tests) {
    try {
      await runTest(plan, tour, instrumented, options);
    } catch (error) {
      if (!(error instanceof Divergence)) throw error;
      firstFailureMs ??= Math.round((performance.now() - start) * 1000) / 1000;
      mismatches.add(error.message.split("\n")[1]);
    }
  }
  return {
    cover: plan.cover,
    tests: tests.length,
    plannedCalls: tests.reduce((sum, test) => sum + test.steps.length, 0),
    calls,
    setups,
    fields: [...fields].sort(),
    mismatches: [...mismatches].sort(),
    firstFailureMs,
    replayMs: Math.round((performance.now() - start) * 1000) / 1000,
  };
}
