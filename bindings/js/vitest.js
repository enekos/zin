// The explicit Vitest entry point attaches replay progress to native test metadata.
// Workers transfer this metadata to the reporter, including on a failed test.
import { createHash } from "node:crypto";
import { basename } from "node:path";
import { describe, test } from "vitest";
import { loadPlan, runTest } from "./index.js";

function* obligations(steps, count) {
  let state = 0;
  let previous;
  for (let index = 0; index < count; index++) {
    const [call, to] = steps[index];
    const operation = `${state}/${call}`;
    yield [operation, previous === undefined ? undefined : `${previous}/${call}`];
    previous = operation;
    if (to >= 0) state = to;
  }
}

/** Register generated tests and report model operations actually attempted. */
export function verify(spec, factory, options = {}) {
  const plan = loadPlan(spec, options);
  const suite = options.name ?? `${plan.system} conforms to ${basename(spec instanceof URL ? spec.pathname : spec)}`;
  const pairs = plan.cover === "pairs";
  const operations = new Set();
  const adjacent = new Set();
  for (const item of plan.tests) {
    for (const [op, pair] of obligations(item.steps, item.steps.length)) {
      operations.add(op);
      if (pairs && pair !== undefined) adjacent.add(pair);
    }
  }
  const model = {
    id: createHash("sha256").update(JSON.stringify(plan)).digest("hex"),
    suite, planned: operations.size,
    eligible: plan.summary.refusals === undefined ? null : plan.summary.refusals + plan.summary.transitions,
    plannedPairs: pairs ? adjacent.size : null, eligiblePairs: plan.summary.eligible_pairs ?? null,
  };
  describe(suite, () => {
    for (const item of plan.tests) {
      test(item.name, { meta: { zin: { model } } }, async ({ task }) => {
        const progress = { attempted: 0, matched: 0, fields: new Set() };
        try {
          await runTest(plan, item, factory, { ...options, progress });
        } finally {
          const done = [...obligations(item.steps, progress.attempted)];
          const failed = progress.attempted > progress.matched ? done.at(-1) : undefined;
          task.meta.zin = { model, executed: [...new Set(done.map(([op]) => op))],
            pairs: pairs ? [...new Set(done.map(([,pair]) => pair).filter(Boolean))] : [],
            failed: failed?.[0], failedPair: pairs ? failed?.[1] : undefined, fields: [...progress.fields] };
        }
      });
    }
  });
  return plan;
}
