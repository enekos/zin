// The runner contract shared by every binding: bindings/fixtures/contract.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { Divergence, PlanError, loadPlan, runTest } from "../index.js";

const CONTRACT = new URL("../../fixtures/contract/", import.meta.url);
const at = (name) => fileURLToPath(new URL(name, CONTRACT));
const json = (name) => JSON.parse(readFileSync(at(name), "utf8"));
const CASES = json("cases.json");
const MISSING_BINARY = at("no-such-zin");
const OPTION_NAMES = {
  cover: "cover",
  max_steps: "maxSteps",
  step_budget: "stepBudget",
  pair_limit: "pairLimit",
  component: "component",
};

for (const item of CASES.load) {
  test(`load: ${item.name}`, () => {
    const options = { bin: MISSING_BINARY };
    for (const [key, value] of Object.entries(item.options ?? {})) options[OPTION_NAMES[key]] = value;
    if (item.plan) options.plan = at(item.plan);
    if (item.expect.errors) {
      assert.throws(
        () => loadPlan(at(item.spec), options),
        (error) => {
          assert.ok(error instanceof PlanError, String(error));
          for (const fragment of item.expect.errors) assert.ok(error.message.includes(fragment), error.message);
          return true;
        },
      );
    } else {
      const plan = loadPlan(at(item.spec), options);
      assert.equal(plan.fingerprint, item.expect.fingerprint);
      assert.equal(plan.cover, item.expect.cover);
    }
  });
}

/** The contract's table-driven adapter (see the contract README). */
function scripted(script, instance) {
  const state = structuredClone(script.initial);
  let calls = 0;
  const call = (action) => {
    calls += 1;
    for (const override of script.overrides ?? []) {
      if (
        (override.instance ?? instance) === instance &&
        (override.call ?? calls) === calls &&
        (override.action ?? action) === action
      ) {
        if (override.outcome === "raise") throw new Error(override.message);
        return override.outcome === "accept";
      }
    }
    const rule = script.rules[action];
    if (Object.entries(rule.requires).some(([key, value]) => state[key] !== value)) return false;
    Object.assign(state, rule.set);
    return true;
  };
  const adapter = {
    actions: Object.fromEntries(Object.keys(script.rules).map((action) => [action, () => call(action)])),
  };
  if (script.observe) adapter.observe = () => ({ ...state });
  return adapter;
}

for (const item of CASES.replay) {
  test(`replay: ${item.name}`, async () => {
    const plan = loadPlan(at(item.plan));
    const script = { ...CASES.adapter, ...item.adapter };
    let made = 0;
    const factory = () => scripted(script, ++made);
    for (const index of item.tests) {
      const run = runTest(plan, plan.tests[index], factory);
      if (item.expect.passes) {
        await run;
      } else if (item.expect.raises) {
        await assert.rejects(run, (error) => !(error instanceof Divergence) && error.message === item.expect.raises);
      } else {
        await assert.rejects(run, (error) => {
          assert.ok(error instanceof Divergence, String(error));
          assert.equal(error.message, item.expect.report);
          return true;
        });
      }
    }
    if (item.expect.factories !== undefined) assert.equal(made, item.expect.factories);
  });
}

// JavaScript has no enum type; enum-like values are their names.
function native(value) {
  if (value !== null && typeof value === "object" && "$enum" in value) return value.$enum;
  if (value !== null && typeof value === "object" && "$set" in value) return new Set(value.$set.map(native));
  if (value !== null && typeof value === "object" && "$record" in value) return native(value.$record);
  if (Array.isArray(value)) return value.map(native);
  if (value !== null && typeof value === "object" && "$map" in value) {
    return new Map(value.$map.map(([key, item]) => [native(key), native(item)]));
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, native(item)]));
  }
  return value;
}

async function matches(expected, observed) {
  const plan = {
    system: "Observation",
    actions: [],
    calls: [],
    states: [expected],
    parents: [null],
    tests: [{ name: "t", kind: "tour", steps: [] }],
  };
  try {
    await runTest(plan, plan.tests[0], () => ({ actions: {}, observe: () => observed }));
    return true;
  } catch (error) {
    if (error instanceof Divergence) return false;
    throw error;
  }
}

for (const item of [...json("observations.json"), ...json("native-observations.json")]) {
  test(`observation: ${item.name}`, async () => {
    assert.equal(await matches(item.expected, native(item.observed)), item.matches);
  });
}

test("plans have every key the schema requires", () => {
  const schema = JSON.parse(readFileSync(new URL("../../../docs/plan.schema.json", import.meta.url), "utf8"));
  for (const name of ["latch.plan.json", "composed/main.plan.json"]) {
    const plan = loadPlan(at(name));
    for (const key of schema.required) assert.ok(key in plan, `${name}: ${key}`);
    for (const key of schema.properties.summary.required) assert.ok(key in plan.summary, `${name}: summary.${key}`);
  }
});
