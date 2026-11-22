const { expect, test } = require("@playwright/test");
const { Divergence, PlanError, check } = require("zin-verify");
const playwright = require("zin-verify/playwright");
const { SPEC, adapter } = require("./editor.cjs");

const { Rejected, actionable, click, fill, gate, verify } = playwright;
const PATIENCE = { observeTimeout: 300 };

const divergence = (factory) =>
  check(SPEC, factory, PATIENCE).then(
    () => null,
    (error) => {
      if (!(error instanceof Divergence)) throw error;
      return error.message;
    },
  );

test("an answer applied to the wrong draft is caught, with the steps that lead to it", async ({ page }) => {
  const message = await divergence(() => adapter("stale")({ page }));
  expect(message).toMatch(/after server_ok, saved is 2, but the specification says 1 \(waited 300 ms\)/);
  expect(message).toMatch(/steps taken:\n  1\. increase\n  2\. save\n  3\. increase\n  4\. server_ok\nspec fingerprint: [0-9a-f]{16}\nscenario "Replay divergence"\n/);
});

test("a second save while one is in flight is caught", async ({ page }) => {
  expect(await divergence(() => adapter("double")({ page }))).toMatch(
    /the implementation accepted save, but the specification rejects it/,
  );
});

test("a failure the page does not show is caught", async ({ page }) => {
  expect(await divergence(() => adapter("silent")({ page }))).toMatch(
    /after server_fails, status is idle, but the specification says failed/,
  );
});

test("a selector that matches nothing fails as a refusal the specification does not allow", async ({ page }) => {
  const broken = async () => {
    const working = await adapter()({ page });
    return { ...working, actions: { ...working.actions, increase: () => click(page.getByRole("button", { name: "Mroe" })) } };
  };
  expect(await divergence(broken)).toMatch(/the implementation rejected increase, but the specification allows it/);
});

test("click and fill refuse what a user could not do, and fail on anything else", async ({ page }) => {
  await page.setContent(`
    <button>Go</button><button disabled>Stop</button><button hidden>Hide</button>
    <input aria-label="Name" /><input aria-label="Locked" readonly /><input aria-label="Off" disabled />`);
  const button = (name) => page.getByRole("button", { name, includeHidden: true });
  expect(await actionable(button("Go"))).toBe(true);
  await click(button("Go"));
  for (const name of ["Stop", "Hide", "Missing"]) {
    expect(await actionable(button(name))).toBe(false);
    await expect(click(button(name))).rejects.toBeInstanceOf(Rejected);
  }
  await fill(page.getByLabel("Name"), 7);
  await expect(page.getByLabel("Name")).toHaveValue("7");
  for (const name of ["Locked", "Off", "Missing"]) {
    await expect(fill(page.getByLabel(name), "x")).rejects.toBeInstanceOf(Rejected);
  }
  const ambiguous = click(page.getByRole("button"));
  await expect(ambiguous).rejects.toThrow(/strict mode violation/);
  await expect(ambiguous).rejects.not.toBeInstanceOf(Rejected);
});

test("a gate holds requests until the test answers them, in the order it chooses", async ({ page }) => {
  await page.route("http://gate.test/", (route) => route.fulfill({ contentType: "text/html", body: "<title>gate</title>" }));
  await page.goto("http://gate.test/");
  const server = await gate(page, "**/api/*", { timeout: 200 });
  const ask = (name) => page.evaluate((path) => {
    window.answers ??= [];
    fetch(path).then(async (response) => window.answers.push(`${path} ${response.status} ${await response.text()}`), () => window.answers.push(`${path} failed`));
  }, `/api/${name}`);

  await expect(server.fulfill()).rejects.toBeInstanceOf(Rejected);
  await server.request(() => ask("first"));
  await server.request(() => ask("second"));
  await server.request(() => ask("third"));
  expect(server.pending).toBe(3);
  expect(server.requests.map((request) => new URL(request.url()).pathname)).toEqual(["/api/first", "/api/second", "/api/third"]);

  await server.fulfill((request) => ({ body: new URL(request.url()).pathname }), (request) => request.url().endsWith("second"));
  await server.fulfill({ status: 503, body: "busy" });
  await server.abort();
  await expect.poll(() => page.evaluate(() => window.answers)).toEqual([
    "/api/second 200 /api/second",
    "/api/first 503 busy",
    "/api/third failed",
  ]);
  await expect(server.abort()).rejects.toBeInstanceOf(Rejected);

  expect(await server.request(() => false)).toBe(false);
  await expect(server.request(() => undefined)).rejects.toThrow(/no request to \*\*\/api\/\* arrived within 200 ms/);

  await server.request(() => ask("left"));
  await server.dispose();
  await expect.poll(() => page.evaluate(() => window.answers.at(-1))).toBe("/api/left failed");
  expect(server.pending).toBe(0);
});

test("verify registers a test per plan test and asks for the fixtures it is told to", async () => {
  const registered = [];
  const fake = Object.assign((name, body) => registered.push([name, body]), {
    describe: (name, body) => { registered.push(name); body(); },
    step: (_text, body) => body(),
  });
  const plan = verify(SPEC, () => ({ actions: {} }), { test: fake, fixtures: ["page", "context"] });
  expect(registered[0]).toBe("Quantity editor conforms to editor.zin");
  expect(registered.length - 1).toBe(plan.tests.length);
  expect(String(registered[1][1])).toMatch(/^async \(\{ page, context \}, testInfo\)/);
  expect(() => verify(SPEC, () => ({ actions: {} }), { test: fake, fixtures: ["page }, x = evil(), { y"] })).toThrow(PlanError);
});

test("the ESM entry is the same binding", async () => {
  const esm = await import("zin-verify/playwright");
  expect(Object.keys(esm).filter((key) => key !== "default").sort()).toEqual(Object.keys(playwright).sort());
  for (const key of Object.keys(playwright)) expect(esm[key]).toBe(playwright[key]);
});
