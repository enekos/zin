// The running application against the specification, in a real browser.
import { test } from "@playwright/test";
import { click, gate, verify } from "zin-verify/playwright";

verify(
  new URL("../../editor.zin", import.meta.url),
  async ({ page }) => {
    // Hold every save: the plan decides when the server answers, and how.
    const server = await gate(page, "**/api/quantity");
    await page.goto("/");
    const button = (name: string) => page.getByRole("button", { name });
    const number = async (id: string) => Number(await page.getByTestId(id).textContent());
    return {
      actions: {
        increase: () => click(button("More")),
        decrease: () => click(button("Less")),
        save: () => server.request(() => click(button("Save"))),
        server_ok: () => server.fulfill({ status: 204 }),
        server_fails: () => server.fulfill({ status: 500 }),
      },
      observe: async () => ({
        draft: await number("draft"),
        saved: await number("saved"),
        status: (await page.getByRole("status").textContent()) ?? "",
      }),
      teardown: () => server.dispose(),
    };
  },
  { test },
);
