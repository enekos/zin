// The explicit Vitest entry point attaches replay progress to native test metadata.
// Workers transfer this metadata to the reporter, including on a failed test.
import coverage from "./coverage.cjs";
import { basename } from "node:path";
import { describe, test } from "vitest";
import { loadPlan, runTest } from "./index.js";

/** Register generated tests and report model operations actually attempted. */
export function verify(spec, factory, options = {}) {
  const plan = loadPlan(spec, options);
  const suite = options.name ?? `${plan.system} conforms to ${basename(spec instanceof URL ? spec.pathname : spec)}`;
  const model = coverage.coverageModel(plan, suite);
  describe(suite, () => {
    for (const item of plan.tests) {
      test(item.name, { meta: { zin: { model } } }, async ({ task }) => {
        const progress = { attempted: 0, matched: 0, fields: new Set() };
        try {
          await runTest(plan, item, factory, { ...options, progress });
        } finally {
          task.meta.zin = coverage.coverageRecord(model, item, progress);
        }
      });
    }
  });
  return plan;
}
