const { createHash } = require("node:crypto");

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

function coverageModel(plan, suite) {
  const pairs = plan.cover === "pairs";
  const operations = new Set();
  const adjacent = new Set();
  for (const item of plan.tests) {
    for (const [op, pair] of obligations(item.steps, item.steps.length)) {
      operations.add(op);
      if (pairs && pair !== undefined) adjacent.add(pair);
    }
  }
  return {
    id: createHash("sha256").update(JSON.stringify(plan)).digest("hex"),
    suite, planned: operations.size,
    eligible: plan.summary.refusals === undefined ? null : plan.summary.refusals + plan.summary.transitions,
    plannedPairs: pairs ? adjacent.size : null, eligiblePairs: plan.summary.eligible_pairs ?? null,
  };
}

function coverageRecord(model, item, progress) {
  const done = [...obligations(item.steps, progress.attempted)];
  const failed = progress.attempted > progress.matched ? done.at(-1) : undefined;
  return { model, executed: [...new Set(done.map(([op]) => op))],
    pairs: model.plannedPairs !== null ? [...new Set(done.map(([,pair]) => pair).filter(Boolean))] : [],
    failed: failed?.[0], failedPair: model.plannedPairs !== null ? failed?.[1] : undefined, fields: [...progress.fields] };
}

function report(records) {
  const groups = new Map();
  for (const { record, source } of records) {
    if (!record) continue;
    const { model } = record;
    const key = JSON.stringify([model.id, model.suite, source]);
    if (!groups.has(key)) groups.set(key, {
      model, executed: new Set(), failed: new Set(), pairs: new Set(), failedPairs: new Set(), fields: new Set(),
    });
    const group = groups.get(key);
    for (const field of ["executed", "pairs", "fields"]) {
      for (const entry of record[field] ?? []) group[field].add(entry);
    }
    if (record.failed) group.failed.add(record.failed);
    if (record.failedPair) group.failedPairs.add(record.failedPair);
  }
  for (const group of groups.values()) {
    const { model } = group;
    const scope = model.eligible == null ? "" : `, ${model.eligible} eligible`;
    const pairs = model.plannedPairs == null ? "" :
      `; pairs ${group.pairs.size}/${model.plannedPairs} planned, ${model.eligiblePairs} eligible, ${group.failedPairs.size} failed`;
    console.log(`Zin ${model.suite}: operations ${group.executed.size}/${model.planned} planned${scope}, ${group.failed.size} failed${pairs}; observed: ${[...group.fields].sort().join(", ") || "(none)"}`);
  }
}

module.exports = { coverageModel, coverageRecord, report };
