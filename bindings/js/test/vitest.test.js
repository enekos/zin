import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

test("Vitest coverage survives worker isolation, name filters and a failed action", () => {
  for (const [selector, status, coverage] of [
    ["Start only", 0, "operations 1/3 planned, 0 failed"],
    ["Cycle", 1, "operations 2/3 planned, 1 failed"],
  ]) {
    const result = spawnSync(process.execPath,
      ["node_modules/vitest/vitest.mjs", "run", "--config", "vitest.coverage.config.js", "-t", selector],
      { cwd: fileURLToPath(new URL("..", import.meta.url)), encoding: "utf8" });
    assert.equal(result.status, status, result.stderr + result.stdout);
    assert.ok(result.stdout.includes(coverage), result.stdout);
    assert.match(result.stdout, /observed: active/);
  }
});
