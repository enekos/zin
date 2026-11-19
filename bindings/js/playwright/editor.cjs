// The adapter between examples/editor.zin and editor.html, shared by the tests.
const { join } = require("node:path");
const { click, gate } = require("zin-verify/playwright");

const SPEC = join(__dirname, "../../../examples/editor.zin");
const ORIGIN = "http://editor.test/";

const adapter = (bug = "") => async ({ page }) => {
  const document = (route) => route.fulfill({ path: join(__dirname, "editor.html") });
  await page.route(`${ORIGIN}?*`, document);
  const server = await gate(page, "**/api/quantity");
  await page.goto(`${ORIGIN}?bug=${bug}`);
  const button = (name) => page.getByRole("button", { name });
  const number = async (id) => Number(await page.getByTestId(id).textContent());
  return {
    actions: {
      increase: () => click(button("More")),
      decrease: () => click(button("Less")),
      save: () => server.request(() => click(button("Save"))),
      server_ok: () => server.fulfill({ json: { ok: true } }),
      server_fails: () => server.fulfill({ status: 500 }),
    },
    observe: async () => ({
      draft: await number("draft"),
      saved: await number("saved"),
      status: await page.getByRole("status").textContent(),
    }),
    teardown: async () => {
      await server.dispose();
      await page.unroute(`${ORIGIN}?*`, document);
    },
  };
};

module.exports = { SPEC, adapter };
