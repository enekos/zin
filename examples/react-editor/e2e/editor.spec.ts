import { test } from "@playwright/test";
import { type Binding, bind, button, oneOf, response, role, testId, verify } from "zin-verify/playwright";
import type { Actions, State } from "./editor-model";

verify(new URL("../../editor.zin", import.meta.url), bind({
  url: "/", requests: "**/api/quantity",
  actions: {
    increase: button("More"), decrease: button("Less"), save: button("Save", { request: true }),
    server_ok: response(204), server_fails: response(500),
  },
  fields: { draft: testId("draft", Number), saved: testId("saved", Number), status: role("status", oneOf("idle", "saving", "failed")) },
} satisfies Binding<Actions, State>), { test });
