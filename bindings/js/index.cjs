// Run a Zin specification's derived test plan against a JavaScript implementation.
//
//   const { verify } = require("zin-verify");
//
//   verify(new URL("./account.zin", import.meta.url), () => {
//     const account = new Account();
//     return {
//       actions: {
//         deposit: ({ amount }) => account.deposit(amount),
//       },
//       rejects: (error) => error instanceof RangeError,
//       observe: () => ({ balance: account.balance }),
//     };
//   });
//
// `verify` registers one test per plan test with the surrounding framework.

const { execFileSync } = require("node:child_process");
const { appendFileSync, existsSync, readFileSync } = require("node:fs");
const { basename, dirname, resolve } = require("node:path");
const { fileURLToPath } = require("node:url");
const recorder = require("./recorder.cjs");

const { plain } = recorder;

const PLAN_FORMAT = "plan/1";
// How many of the latest steps a report lists before eliding earlier ones.
const LISTED_STEPS = 8;
// Pause between two observations while waiting for the expected state.
const OBSERVE_INTERVAL_MS = 20;

/** Throw from an action to say the implementation refused it. */
class Rejected extends Error {
  constructor(message = "rejected") {
    super(message);
    this.name = "Rejected";
  }
}

/** The implementation does not behave as the specification says. */
class Divergence extends Error {
  constructor(message, fingerprint, scenario) {
    super(message);
    this.name = "Divergence";
    this.fingerprint = fingerprint;
    this.scenario = scenario;
  }
}

/** The test plan could not be loaded or derived. */
class PlanError extends Error {
  constructor(message) {
    super(message);
    this.name = "PlanError";
  }
}

/** FNV-1a 64 of the specification bytes, as `zin plan` records it. */
function fingerprint(bytes) {
  let value = 0xcbf29ce484222325n;
  for (const byte of bytes) {
    value = ((value ^ BigInt(byte)) * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return value.toString(16).padStart(16, "0");
}

function toPath(location) {
  if (location instanceof URL) return fileURLToPath(location);
  if (typeof location === "string" && location.startsWith("file:")) return fileURLToPath(location);
  return String(location);
}

function readPlan(path) {
  let plan;
  try {
    plan = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new PlanError(`cannot read plan ${path}: ${error.message}`);
  }
  if (plan === null || typeof plan !== "object" || plan.zin !== PLAN_FORMAT) {
    throw new PlanError(`${path} is not a ${PLAN_FORMAT} file`);
  }
  return plan;
}

/**
 * The plan for `spec`: `options.plan` when given, else a sibling
 * `<name>.plan.json` that matches the specification, else the output of
 * `zin plan` (`options.bin`, `$ZIN_BIN`, or `zin` on PATH). `spec` may itself
 * be a plan file.
 */
function loadPlan(spec, options = {}) {
  const path = toPath(spec);
  if (path.endsWith(".json")) return readPlan(path);
  const cover = options.cover ?? "transitions";
  let wanted;
  try {
    wanted = fingerprint(readFileSync(path));
  } catch (error) {
    throw new PlanError(`cannot read specification ${path}: ${error.message}`);
  }
  const defaults = { max_steps: 200, step_budget: 2_000_000, pair_limit: null };
  const settings = { max_steps: options.maxSteps, step_budget: options.stepBudget, pair_limit: options.pairLimit };
  const limit = [];
  for (const [key, value] of Object.entries(settings)) {
    if (value === undefined) continue;
    if (!Number.isSafeInteger(value) || value < 1) {
      throw new PlanError(`${key} must be a positive integer`);
    }
    limit.push("--" + key.replaceAll("_", "-"), String(value));
  }
  if (options.component !== undefined) limit.push("--component", options.component);
  const explicit = options.plan !== undefined;
  const sibling = path.replace(/\.[^./\\]+$/, "") + ".plan.json";
  const cached = explicit ? toPath(options.plan) : sibling;
  let stale = false;
  if (explicit || existsSync(cached)) {
    const plan = readPlan(cached);
    let fresh = plan.fingerprint === wanted;
    try {
      fresh &&= Object.entries(plan.sources || {}).every(([file, digest]) =>
        fingerprint(readFileSync(resolve(dirname(path), file))) === digest);
    } catch {
      fresh = false;
    }
    const componentMatches = (explicit && options.component === undefined) || (plan.component ?? null) === (options.component ?? null);
    const compatible = componentMatches && ((explicit && options.cover === undefined) || plan.cover === cover) &&
      Object.entries(settings).every(([key, value]) => (explicit && value === undefined) ||
        (plan[key] ?? defaults[key]) === (value ?? defaults[key]));
    if (fresh && compatible) return plan;
    if (explicit) {
      const reason = fresh ? "has different planning options" : `was derived from a different version of ${basename(path)}`;
      throw new PlanError(`${cached} ${reason}; regenerate with: zin plan ${path} --cover ${cover} ${limit.join(" ")} --out ${cached}`);
    }
    stale = true;
  }
  const binary = options.bin || process.env.ZIN_BIN || "zin";
  let output;
  try {
    output = execFileSync(binary, ["plan", path, "--cover", cover, ...limit], {
      encoding: "utf8",
      maxBuffer: 1024 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    if (error.code === "ENOENT") {
      const hint = stale ? `${basename(sibling)} is out of date and ` : "";
      throw new PlanError(
        `${hint}the zin binary was not found. Install it (cargo install zin-spec), set ZIN_BIN, ` +
          `or commit a plan: zin plan ${path} --cover ${cover}${limit.length ? ` ${limit.join(" ")}` : ""} --out ${sibling}`,
      );
    }
    throw new PlanError(`zin plan ${path} failed:\n${String(error.stderr ?? error.message).trim()}`);
  }
  const plan = JSON.parse(output);
  if (plan.zin !== PLAN_FORMAT) {
    throw new PlanError(`${binary} produced ${plan.zin}, but this package reads ${PLAN_FORMAT}`);
  }
  return plan;
}

function show(value) {
  if (value !== null && typeof value === "object") {
    return `{${Object.entries(value)
      .map(([key, item]) => `${key}: ${show(item)}`)
      .join(", ")}}`;
  }
  return String(value);
}

function same(expected, observed) {
  if (expected !== null && typeof expected === "object") {
    return (
      observed !== null &&
      Object.prototype.toString.call(observed) === "[object Object]" &&
      Object.entries(observed).every(([key, item]) => Object.hasOwn(expected, key) && same(expected[key], item))
    );
  }
  return expected === observed;
}

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

class Runner {
  constructor(plan, options = {}) {
    this.plan = plan;
    this.step = options.step ?? ((_text, body) => body());
    this.observeTimeout = options.observeTimeout ?? 0;
    this.observeInterval = options.observeInterval ?? OBSERVE_INTERVAL_MS;
    this.params = new Map(
      plan.actions.map((action) => [action.name, action.params.map((param) => param.name)]),
    );
  }

  text([call, to]) {
    const text = this.plan.calls[call].text;
    return to >= 0 ? text : `reject ${text}`;
  }

  listing(steps) {
    const skipped = Math.max(0, steps.length - LISTED_STEPS);
    const lines = skipped ? [`  ... ${skipped} earlier steps`] : [];
    for (let index = skipped; index < steps.length; index++) {
      lines.push(`  ${index + 1}. ${this.text(steps[index])}`);
    }
    return lines.join("\n");
  }

  /** One observation: `{ message, waitable }`; only a wrong value can still become right. */
  async mismatch(adapter, state, after, progress) {
    const observed = await adapter.observe();
    if (observed === null || observed === undefined) return null;
    const fields = plain(observed);
    if (Object.prototype.toString.call(fields) !== "[object Object]") {
      return { message: `observe() must return an object of state fields, not ${typeof observed}` };
    }
    const expected = this.plan.states[state];
    for (const [field, raw] of Object.entries(fields)) {
      if (!Object.hasOwn(expected, field)) {
        return { message: `observe() returned '${field}', which is not a state field of the specification` };
      }
      progress?.fields.add(field);
      if (!same(expected[field], raw)) {
        return {
          message: `${after}, ${field} is ${show(raw)}, but the specification says ${show(expected[field])}`,
          waitable: true,
        };
      }
    }
    return null;
  }

  /** Compares what the adapter observes with a model state, waiting up to `observeTimeout` for it to match. */
  async compare(adapter, state, after, progress) {
    if (typeof adapter.observe !== "function") return null;
    const deadline = Date.now() + this.observeTimeout;
    for (;;) {
      const found = await this.mismatch(adapter, state, after, progress);
      if (found === null) return null;
      if (!found.waitable || this.observeTimeout <= 0) return found.message;
      if (Date.now() >= deadline) return `${found.message} (waited ${this.observeTimeout} ms)`;
      await sleep(this.observeInterval);
    }
  }

  async perform(adapter, call) {
    const action = adapter.actions?.[call.action];
    if (typeof action !== "function") {
      throw new PlanError(`the adapter has no function for the action '${call.action}'`);
    }
    const names = this.params.get(call.action);
    const args = Object.fromEntries(call.args.map((value, index) => [names[index], value]));
    try {
      return (await action.call(adapter.actions, args)) !== false;
    } catch (error) {
      if (error instanceof Rejected || (typeof adapter.rejects === "function" && adapter.rejects(error))) {
        return false;
      }
      throw error;
    }
  }

  /** The `[step number, message]` of the first divergence, if any. */
  async run(adapter, steps, progress) {
    let message = await this.compare(adapter, 0, "before any action", progress);
    if (message) return [0, message];
    let state = 0;
    for (const [index, step] of steps.entries()) {
      const [callId, to] = step;
      const call = this.plan.calls[callId];
      message = await this.step(this.text(step), async () => {
        if (progress) progress.attempted = index + 1;
        const accepted = await this.perform(adapter, call);
        if (typeof adapter.settle === "function") await adapter.settle();
        if (to >= 0 && accepted) {
          state = to;
          return this.compare(adapter, state, `after ${call.text}`, progress);
        }
        if (to < 0 && !accepted) return this.compare(adapter, state, `after rejecting ${call.text}`, progress);
        return accepted
          ? `the implementation accepted ${call.text}, but the specification rejects it`
          : `the implementation rejected ${call.text}, but the specification allows it`;
      });
      if (message) return [index + 1, message];
      if (progress) progress.matched = index + 1;
    }
    return null;
  }

  shortest(steps, failed) {
    let state = 0;
    for (const [, to] of steps.slice(0, failed - 1)) {
      if (to >= 0) state = to;
    }
    const path = [];
    while (this.plan.parents[state] !== null) {
      const [parent, call] = this.plan.parents[state];
      path.push([call, state]);
      state = parent;
    }
    path.reverse();
    path.push(steps[failed - 1]);
    return path;
  }

  async withAdapter(factory, body) {
    const adapter = await factory();
    try {
      return await body(adapter);
    } finally {
      if (typeof adapter.teardown === "function") await adapter.teardown();
    }
  }

  async check(test, factory, progress) {
    const steps = test.steps;
    const found = await this.withAdapter(factory, (adapter) => this.run(adapter, steps, progress));
    if (found === null) return;
    const [failed, message] = found;
    const taken = steps.slice(0, failed);
    let report =
      `${this.plan.system}: implementation diverges from the specification ` +
      `in test "${test.name}" at step ${failed}\n${message}\nsteps taken:\n${this.listing(taken)}`;
    const scenario = ['scenario "Replay divergence"', ...(taken.length
      ? taken.map(step => "  " + this.text(step)) : ["  expect true"])].join("\n");
    report += `\nspec fingerprint: ${this.plan.fingerprint}\n${scenario}`;
    if (failed > 0) {
      const short = this.shortest(steps, failed);
      if (short.length < taken.length) {
        let again;
        try {
          again = await this.withAdapter(factory, (adapter) => this.run(adapter, short));
        } catch (error) {
          report += `\nshorter replay failed: ${error.message ?? error}`;
        }
        report += again && again[0] === short.length && again[1] === message
          ? `\nshortest reproduction:\n${this.listing(short.slice(0, again[0]))}`
          : "\nthe shortest path to the same specification state does not reproduce the original mismatch";
      }
    }
    throw new Divergence(report, this.plan.fingerprint, scenario);
  }
}

/** Runs one plan test against fresh adapters from `factory`. */
async function runTest(plan, test, factory, options = {}) {
  await new Runner(plan, options).check(test, factory, options.progress);
}

/** Runs every plan test; rejects with `Divergence` on the first failure. */
async function check(spec, factory, options = {}) {
  const plan = loadPlan(spec, options);
  const runner = new Runner(plan, options);
  for (const test of plan.tests) {
    await runner.check(test, factory);
  }
  return plan.summary;
}

/**
 * Registers one test per plan test with the test framework: the `describe`
 * and `test` passed in `options`, or the globals Jest, Vitest (with globals)
 * and Mocha provide.
 */
function verify(spec, factory, options = {}) {
  const describe = options.describe ?? globalThis.describe;
  const test = options.test ?? options.it ?? globalThis.test ?? globalThis.it;
  if (typeof describe !== "function" || typeof test !== "function") {
    throw new PlanError(
      "no test framework globals found; pass them in: verify(spec, factory, { describe, test })",
    );
  }
  const plan = loadPlan(spec, options);
  const runner = new Runner(plan, options);
  describe(options.name ?? `${plan.system} conforms to ${basename(toPath(spec))}`, () => {
    for (const planTest of plan.tests) {
      test(planTest.name, () => runner.check(planTest, factory));
    }
  });
  return plan;
}

/** Writes the JSON-lines event log that `zin trace <spec> <log>` checks. */
/** The recorder, which here also appends to a file given by path or URL. */
class Recorder extends recorder.Recorder {
  constructor(target) {
    super(
      target === undefined || typeof target === "function"
        ? target
        : (line) => appendFileSync(toPath(target), line),
    );
  }
}

module.exports = { PLAN_FORMAT, Rejected, Divergence, PlanError, fingerprint, loadPlan, runTest, check, verify, Recorder };
