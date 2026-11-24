// Run a Zin specification's derived test plan against a page in Playwright Test.
//
//   const { test } = require("@playwright/test");
//   const { verify, click, gate } = require("zin-verify/playwright");
//
//   verify("editor.zin", async ({ page }) => {
//     const server = await gate(page, "**/api/quantity");
//     await page.goto("/");
//     return {
//       actions: {
//         save: () => server.request(() => click(page.getByRole("button", { name: "Save" }))),
//         server_ok: () => server.fulfill({ json: { ok: true } }),
//       },
//       observe: async () => ({ status: await page.getByRole("status").textContent() }),
//       teardown: () => server.dispose(),
//     };
//   }, { test });
//
// `verify` registers one Playwright test per plan test; every step is a `test.step`.

const { basename, dirname, resolve } = require("node:path");
const { fileURLToPath } = require("node:url");
const { coverageModel, coverageRecord } = require("./coverage.cjs");
const { hermetic } = require("./hermetic.cjs");
const { Channel, socketFrames, eventStream, clock, navigation, connectivity, actors } = require("./channels.cjs");
const { binding, button, response, role, oneOf, testId } = require("./bind.cjs");
const { PlanError, Rejected, loadPlan, runTest } = require("./index.cjs");

// How long an observation may take to match the specification: Playwright's own default for `expect`.
const OBSERVE_TIMEOUT_MS = 5000;
// How long a request an action should have sent may take to arrive.
const REQUEST_TIMEOUT_MS = 5000;
const FIXTURE_NAME = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

/**
 * A test function that asks Playwright for `fixtures`. Playwright decides
 * which fixtures to set up by reading the parameter list of the function's
 * source, so the names have to be written into it.
 */
function testBody(fixtures, run) {
  for (const name of fixtures) {
    if (typeof name !== "string" || !FIXTURE_NAME.test(name)) {
      throw new PlanError(`'${name}' is not a fixture name`);
    }
  }
  const names = fixtures.join(", ");
  return new Function("run", `return async ({ ${names} }, testInfo) => run({ ${names} }, testInfo);`)(run);
}

function playwrightTest(options) {
  if (options.test !== undefined) return options.test;
  try {
    return require("@playwright/test").test;
  } catch (error) {
    throw new PlanError(`@playwright/test could not be loaded (${error.message}); pass its test: verify(spec, factory, { test })`);
  }
}

/**
 * Registers one Playwright test per plan test. The factory receives the
 * fixtures named in `options.fixtures` (default `["page"]`) and the test
 * info, and returns the adapter. It runs again for a shortest reproduction,
 * in the same test: start the application from scratch there.
 */
function verify(spec, factory, options = {}) {
  const test = playwrightTest(options);
  const planFile = String(spec).endsWith(".json");
  const plan = loadPlan(spec, factory.fields && !planFile ? { ...options, observe: factory.fields } : options);
  if (factory.fields && planFile && (!Array.isArray(plan.observe) || plan.observe.length !== factory.fields.length ||
      !factory.fields.every(field => plan.observe.includes(field)))) {
    throw new PlanError("the plan's observation must match the binding fields");
  }
  const path = spec instanceof URL || String(spec).startsWith("file:") ? fileURLToPath(spec) : String(spec);
  const run = {
    observeTimeout: options.observeTimeout ?? OBSERVE_TIMEOUT_MS,
    observeInterval: options.observeInterval,
    location(action) {
      const at = plan.action_locations?.[action];
      return at && { ...at, file: resolve(dirname(path), at.file) };
    },
    step: (text, body, location) => test.step(text, body, { location }),
  };
  const suite = options.name ?? `${plan.system} conforms to ${basename(path)}`;
  const model = options.coverage && coverageModel(plan, suite);
  test.describe(suite, () => {
    if (options.hermetic) test.use({ serviceWorkers: "block" });
    for (const planTest of plan.tests) {
      test(
        planTest.name,
        testBody(options.fixtures ?? ["page"], async (fixtures, testInfo) => {
          const progress = model ? { attempted: 0, matched: 0, fields: new Set() } : undefined;
          const guard = options.hermetic && await hermetic(fixtures.page, options.hermetic);
          try {
            await runTest(plan, planTest, () => factory(fixtures, testInfo), { ...run, progress });
          } finally {
            if (model) await testInfo.attach("zin-coverage", {
              contentType: "application/json",
              body: Buffer.from(JSON.stringify(coverageRecord(model, planTest, progress))),
            });
            if (guard) await guard.dispose();
          }
        }),
      );
    }
  });
  return plan;
}

/** Whether a user could act on the element right now: it is on screen and enabled. Never waits. */
async function actionable(locator) {
  return (await locator.isVisible()) && (await locator.isEnabled());
}

/** Clicks; a control that is missing, hidden or disabled refuses the action. */
async function click(locator, options) {
  if (!(await actionable(locator))) throw new Rejected(`${locator} cannot be clicked`);
  await locator.click(options);
}

/** Fills; a field that is missing, hidden, disabled or read-only refuses the action. */
async function fill(locator, value, options) {
  if (!((await locator.isVisible()) && (await locator.isEditable()))) {
    throw new Rejected(`${locator} cannot be filled`);
  }
  await locator.fill(String(value), options);
}

/**
 * Holds every request to a URL until the test decides its outcome, which
 * turns "the server answers" into an action the plan can order like any other.
 */
class Gate extends Channel {
  constructor(target, url, options = {}) {
    super(`request to ${url}`);
    this.target = target;
    this.url = url;
    this.timeout = options.timeout ?? REQUEST_TIMEOUT_MS;
    this.arrived = 0;
    this.waiting = [];
    this.handler = (route) => {
      this.hold(route);
      this.arrived += 1;
      for (const wake of this.waiting.splice(0)) wake();
    };
  }

  /** The held requests, oldest first. */
  get requests() {
    return this.held.map((route) => route.request());
  }

  /**
   * Performs an action that should send a request, and returns once the gate
   * holds it. A refused action stays refused; an accepted one that sends
   * nothing is an error.
   */
  async request(action) {
    const before = this.arrived;
    const result = await action();
    if (result === false) return false;
    const deadline = Date.now() + this.timeout;
    while (this.arrived === before) {
      const left = deadline - Date.now();
      if (left <= 0) throw new Error(`no request to ${this.url} arrived within ${this.timeout} ms`);
      let timer;
      await new Promise((resolve) => {
        this.waiting.push(resolve);
        timer = setTimeout(() => {
          this.waiting = this.waiting.filter(wake => wake !== resolve);
          resolve();
        }, left);
      });
      clearTimeout(timer);
    }
    return result;
  }

  take(match) {
    return super.take(match && (route => match(route.request())));
  }

  /**
   * Answers the oldest held request, or the oldest that `match` accepts.
   * `response` is what `route.fulfill` takes, or a function from the request
   * to it. With nothing to answer, the action is refused.
   */
  async fulfill(response = {}, match) {
    const route = this.take(match);
    await route.fulfill(typeof response === "function" ? await response(route.request()) : response);
  }

  /** Fails the oldest held request (or the oldest `match` accepts) at the network level. */
  async abort(errorCode, match) {
    await this.take(match).abort(errorCode);
  }

  /** Lets the oldest held request (or the oldest `match` accepts) through to the real server. */
  async continue(overrides, match) {
    await this.take(match).continue(overrides);
  }

  /** Stops holding requests and drops the held ones. Call it from `teardown`. */
  async dispose() {
    await this.target.unroute(this.url, this.handler);
    for (const route of this.held.splice(0)) await route.abort().catch(() => {});
  }
}

/** Starts holding requests to `url` (a glob, RegExp or predicate) on a page or browser context. */
async function gate(target, url, options) {
  const held = new Gate(target, url, options);
  await target.route(url, held.handler);
  return held;
}

const bind = binding({ gate, click });
module.exports = { Channel, socketFrames, eventStream, clock, navigation, connectivity, actors, bind, button, response, role, oneOf, testId, Rejected, Gate, verify, actionable, click, fill, gate };
