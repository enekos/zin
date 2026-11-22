import { defineConfig } from "vitest/config";
import ZinReporter from "zin-verify/vitest-reporter";

export default defineConfig({ test: { include: ["vitest/*.test.js"], reporters: ["default", new ZinReporter()] } });
