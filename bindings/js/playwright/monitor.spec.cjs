const { test, expect } = require("@playwright/test");
const { runTest } = require("zin-verify");
const { monitor } = require("../monitor.cjs");
const { hermetic } = require("../hermetic.cjs");

test("a monitor catches a flicker between two matching observations", async ({ page }) => {
  const plan = {
    system: "Counter", actions: [{ name: "add", params: [] }],
    calls: [{ action: "add", args: [], text: "add" }],
    states: [{ count: 0 }, { count: 1 }], parents: [null, [0, 0]],
  };
  const item = { name: "counter", steps: [[0, 1]] };
  const read = () => ({ count: Number(document.querySelector("output").value) });
  const factory = async () => {
    await page.setContent("<output>0</output>");
    return { actions: { add: () => page.evaluate(async () => {
      const output = document.querySelector("output");
      output.value = 9;
      await new Promise(resolve => requestAnimationFrame(resolve));
      output.value = 1;
    }) }, observe: () => page.evaluate(read) };
  };
  await runTest(plan, item, factory);
  await expect(runTest(plan, item, factory, { monitor: () => monitor(page, read) }))
    .rejects.toThrow(/intermediate observation .*"count":9/);
});

test("an unexpected fetch fails hermetic replay even when the app catches it", async ({ page }) => {
  const guard = await hermetic(page);
  await page.route("http://app.test/", route => route.fulfill({ contentType: "text/html", body: "<title>App</title>" }));
  await page.goto("http://app.test/");
  const sample = () => page.evaluate(() => Math.random());
  const initial = await sample();
  await page.reload();
  expect(await sample()).toBe(initial);
  await page.evaluate(() => fetch("/unguarded").catch(() => {}));
  await expect(guard.dispose()).rejects.toThrow(/ungated API request: GET .*unguarded/);
});
