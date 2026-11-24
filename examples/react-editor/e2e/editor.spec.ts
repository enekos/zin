import { test } from "@playwright/test";
import { type Binding, bind, button, oneOf, response, text, verify } from "zin-verify/playwright";
import { inspectState } from "./probes";
import type { Actions, State } from "./editor-model";

verify(new URL("../../editor.zin", import.meta.url), bind({
  url: "/", requests: "**/api/quantity",
  actions: {
    increase: button("More"), decrease: button("Less"), save: button("Save", { request: true }),
    server_ok: response(204), server_fails: response(500),
  },
  fields: { draft: text("[data-testid=draft]", Number), saved: text("[data-testid=saved]", Number), status: text("[role=status]", oneOf("idle", "saving", "failed")) },
} satisfies Binding<Actions, State>), { test, coverage: true, hermetic: true, onState: inspectState });
