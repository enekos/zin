import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import * as esm from "zin-verify";

const require = createRequire(import.meta.url);

test("CommonJS and ESM share their API and error classes", () => {
  const cjs = require("zin-verify");
  assert.deepEqual(Object.keys(esm).sort(), Object.keys(cjs).sort());
  for (const key of Object.keys(esm)) assert.equal(esm[key], cjs[key]);
});

test("Jest reports individual generated failures and exits unsuccessfully", () => {
  const result = spawnSync(process.execPath, [require.resolve("jest/bin/jest"),
    "--runInBand", "--json", "--config", JSON.stringify({
      rootDir: fileURLToPath(new URL("..", import.meta.url)),
      testEnvironment: "node", transform: {}, testMatch: ["<rootDir>/jest/broken.fixture.cjs"],
    }),
  ], { encoding: "utf8" });
  assert.equal(result.status, 1, result.stderr);
  const report = JSON.parse(result.stdout);
  assert.ok(report.numFailedTests > 0);
  assert.ok(report.numPassedTests > 0);
  const failure = report.testResults[0].assertionResults.find((test) => test.status === "failed");
  assert.match(failure.fullName, /Bank account conforms to account.zin/);
  assert.match(failure.failureMessages.join("\n"), /implementation accepted deposit.*specification rejects it/);
  assert.match(failure.failureMessages.join("\n"), /steps taken:/);
});
