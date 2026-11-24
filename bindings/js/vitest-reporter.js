import coverage from "./coverage.cjs";

export default class ZinReporter {
  onTestRunEnd(modules) {
    const records = [];
    for (const module of modules) {
      for (const item of module.children.allTests()) records.push({ record: item.meta().zin, source: module.moduleId });
    }
    coverage.report(records);
  }
}
