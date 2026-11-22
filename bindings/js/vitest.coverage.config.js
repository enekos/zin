import ZinReporter from "zin-verify/vitest-reporter";
export default { test: { include: ["vitest/coverage.fixture.js"], reporters: [new ZinReporter()] } };
