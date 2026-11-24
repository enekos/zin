import { test } from "@playwright/test";
import { fileURLToPath } from "node:url";
import { check } from "zin-verify";
import { bind, button, response, text } from "zin-verify/playwright";
import editor from "../../bindings/js/playwright/editor.cjs";

const options = { observe: ["draft", "saved", "status"], observeTimeout: 5000 };
const document = fileURLToPath(new URL("../../bindings/js/playwright/editor.html", import.meta.url));

test("function adapter", async ({ page }) => {
  await check(editor.SPEC, () => editor.adapter()({ page }), options);
});

test("declarative adapter", async ({ page }) => {
  const url = "http://editor.test/";
  await page.route(url, route => route.fulfill({ path: document }));
  const factory = bind({
    url, requests: "**/api/quantity",
    actions: { increase: button("More"), decrease: button("Less"), save: button("Save", { request: true }),
      server_ok: response(204), server_fails: response(500) },
    fields: { draft: text("#draft", Number), saved: text("#saved", Number), status: text("#status") },
  });
  await check(editor.SPEC, () => factory({ page }), options);
});
