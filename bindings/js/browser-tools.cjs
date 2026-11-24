// Invoked by the Rust CLI from the consuming project, where Playwright is installed.
const { basename, resolve } = require("node:path");
const { pathToFileURL } = require("node:url");
const { loadPlan, runTest } = require("./index.cjs");

async function scaffold(plan, page, spec, url) {
  await page.goto(url);
  const actions = [];
  for (const { name, params } of plan.actions) {
    const words = name.replaceAll("_", " ");
    const candidates = [...new Set([words, words.charAt(0).toUpperCase() + words.slice(1), name])];
    let action;
    for (const label of candidates) {
      if (!params.length && await page.getByRole("button", { name: label, exact: true }).count() === 1) {
        action = `button(${JSON.stringify(label)})`;
        break;
      }
    }
    action ??= `() => { throw new Error(${JSON.stringify(`bind action ${name}`)}); }`;
    actions.push(`    ${JSON.stringify(name)}: ${action},`);
  }
  const fields = [];
  for (const { name, type } of plan.fields) {
    if (!["enum", "integer"].includes(type.kind)) continue;
    const parse = type.kind === "integer" ? ", Number" : "";
    if (await page.getByTestId(name).count() === 1) {
      fields.push(`    ${JSON.stringify(name)}: testId(${JSON.stringify(name)}${parse}),`);
    } else if (name === "status" && await page.getByRole("status").count() === 1) {
      fields.push(`    "status": role("status"${parse}),`);
    }
  }
  return [
    'import { test } from "@playwright/test";',
    'import { bind, button, role, testId, verify } from "zin-verify/playwright";', "",
    `verify(new URL(${JSON.stringify("./" + basename(spec))}, import.meta.url), bind({`,
    `  url: ${JSON.stringify(url)},`, "  actions: {", ...actions, "  },",
    "  fields: {", ...fields, "  },", "}), { test });",
  ].join("\n");
}

async function diagnose(plan, browser, binding) {
  const factory = (await import(pathToFileURL(resolve(binding)))).default;
  if (typeof factory !== "function") throw new Error("the binding module must default-export an adapter factory");
  const findings = new Set();
  const missingButtons = new Map(Object.entries(factory.buttons ?? {}));
  const readings = new Set();
  let observations = 0;
  for (const item of plan.tests) {
    const page = await browser.newPage();
    try {
      await runTest(plan, item, async () => {
        const adapter = await factory({ page, context: page.context(), browser });
        for (const { name } of plan.actions) {
          if (typeof adapter.actions?.[name] !== "function") findings.add(`unbound action: ${name}`);
        }
        // Sample before actions as well, so a control missing on its very
        // first accepted call can be distinguished from a disabled one.
        async function inspect() {
          for (const [key, label] of missingButtons) {
            if (await page.getByRole("button", { name: label, exact: true }).count()) missingButtons.delete(key);
          }
          const observed = await adapter.observe?.();
          if (observed != null) {
            observations++;
            if (readings.size < 2) readings.add(JSON.stringify(observed, (_k, v) => v instanceof Map ? Object.fromEntries(v) : v));
          }
          return observed;
        }
        return {
          actions: adapter.actions, observe: inspect, rejects: adapter.rejects?.bind(adapter),
          settle: adapter.settle?.bind(adapter), teardown: adapter.teardown?.bind(adapter),
        };
      }, { observeTimeout: 300 });
    } catch (error) {
      findings.add(error.message.split("\n").slice(0, 2).join("\n"));
    } finally { await page.close(); }
  }
  for (const [key, label] of missingButtons) findings.add(`${key}: button ${JSON.stringify(label)} never matched in visited states`);
  if (plan.states.length > 1 && observations > 1 && readings.size === 1) findings.add("observe() returned a constant projection across replay");
  return [...findings];
}

exports.main = async ([command, spec, argument]) => {
  const { chromium } = require("@playwright/test");
  const plan = loadPlan(spec, command === "init" ? { cover: "scenarios" } : {});
  const browser = await chromium.launch();
  try {
    if (command === "init") {
      console.log(await scaffold(plan, await browser.newPage(), spec, argument));
    } else {
      const findings = await diagnose(plan, browser, argument);
      for (const finding of findings) console.log(finding);
      if (findings.length) process.exitCode = 1;
      else console.log(`${plan.system}: ${plan.tests.length} browser tests passed`);
    }
  } finally { await browser.close(); }
};
