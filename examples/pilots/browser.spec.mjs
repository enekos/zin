import { expect, test } from "@playwright/test";
import { loadPlan } from "zin-verify";
import editor from "../../bindings/js/playwright/editor.cjs";
import { measure } from "./measure.mjs";

// Use the same scenario on both implementations and hold the HTTP response until the
// model releases it. This separates the adapter's cost from plan generation and launch.
for (const broken of [false, true]) {
  test(broken ? "detect a stale save" : "accept a consistent save", async ({ page }, info) => {
    const plan = loadPlan(editor.SPEC, { cover: "scenarios" });
    const tests = plan.tests.filter(tour => tour.name.includes("An answer is about what was sent"));
    expect(tests).toHaveLength(1);
    const createAdapter = editor.adapter(broken ? "stale" : "");
    const row = await measure(plan, tests, () => createAdapter({ page }), { observeTimeout: 350 });
    console.log(JSON.stringify({ pilot: "browser save while editing", variant: broken ? "faulty" : "fixed", ...row }));
    await info.attach("pilot.json", { body: JSON.stringify(row, null, 2), contentType: "application/json" });
    if (broken) expect(row.mismatches.join("\n")).toContain("after server_ok, saved is 2");
    else expect(row.mismatches).toEqual([]);
  });
}
