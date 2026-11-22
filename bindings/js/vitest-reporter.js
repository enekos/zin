/** A Vitest reporter for the tests registered by zin-verify/vitest. */
export default class ZinReporter {
  onTestRunEnd(modules) {
    const groups = new Map();
    for (const module of modules) {
      for (const item of module.children.allTests()) {
        const record = item.meta().zin;
        if (!record) continue;
        const { model } = record;
        const key = JSON.stringify([model.id, model.suite, module.moduleId]);
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
    }
    for (const group of groups.values()) {
      const { model } = group;
      const scope = model.eligible == null ? "" : `, ${model.eligible} eligible`;
      const pairs = model.plannedPairs == null ? "" :
        `; pairs ${group.pairs.size}/${model.plannedPairs} planned, ${model.eligiblePairs} eligible, ${group.failedPairs.size} failed`;
      console.log(`Zin ${model.suite}: operations ${group.executed.size}/${model.planned} planned${scope}, ${group.failed.size} failed${pairs}; observed: ${[...group.fields].sort().join(", ") || "(none)"}`);
    }
  }
}
