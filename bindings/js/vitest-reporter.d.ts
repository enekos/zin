import type { Reporter, TestModule } from "vitest/node";
export default class ZinReporter implements Reporter {
  onTestRunEnd(modules: ReadonlyArray<TestModule>): void;
}
