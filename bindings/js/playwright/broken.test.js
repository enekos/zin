import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const require = createRequire(import.meta.url);

test("Playwright reports individual generated failures and exits unsuccessfully", () => {
  const result = spawnSync(process.execPath, [require.resolve("@playwright/test/cli"),
    "test", "--config", fileURLToPath(new URL("../playwright.broken.config.cjs", import.meta.url)), "--reporter", "json",
  ], { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  assert.equal(result.status, 1, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.ok(report.stats.unexpected > 0);
  assert.ok(report.stats.expected > 0);
  const [suite] = report.suites[0].suites;
  assert.equal(suite.title, "Quantity editor conforms to editor.zin");
  const outcomes = suite.specs.filter((spec) => !spec.ok).map((spec) => spec.tests[0].results[0]);
  const [outcome] = outcomes;
  assert.match(outcome.error.message, /after server_ok, saved is 2, but the specification says 1/);
  assert.match(outcome.error.message, /editor\.zin:48:1/);
  assert.ok(outcomes.some((failure) => /shortest reproduction:/.test(failure.error.message)));
  assert.ok(outcome.steps.some((step) => step.title === "server_ok"));
  assert.ok(outcomes.some((failure) => failure.steps.some((step) => step.title.startsWith("reject "))));
});
