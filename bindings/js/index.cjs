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
const { basename, dirname, join } = require("node:path");
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

/** A conformance failure with its original, pasteable Zin scenario. */
class Divergence extends Error {
  constructor(message, fingerprint = "", scenario = "") {
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

// The planning options a caller may pass: option name, plan key, `zin plan` flag.
const OPTIONS = [
  ["cover", "cover", "--cover"],
  ["maxSteps", "max_steps", "--max-steps"],
  ["stepBudget", "step_budget", "--step-budget"],
  ["pairLimit", "pair_limit", "--pair-limit"],
  ["component", "component", "--component"],
];

const byCodePoint = (a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b));

/**
 * The fingerprint `zin plan` would record now, or null if a file is unreadable.
 * With included files, the entry file's hash and each source's current hash,
 * sorted by path, are hashed together (see docs/plan-format.md).
 */
function currentFingerprint(spec, sources) {
  try {
    const entry = fingerprint(readFileSync(spec));
    const paths = Object.keys(sources ?? {}).sort(byCodePoint);
    if (paths.length === 0) return entry;
    let text = `${entry}\n`;
    for (const path of paths) text += `${path}\t${fingerprint(readFileSync(join(dirname(spec), path)))}\n`;
    return fingerprint(Buffer.from(text, "utf8"));
  } catch {
    return null;
  }
}

const fresh = (plan, spec) => currentFingerprint(spec, plan.sources) === plan.fingerprint;

/** Only options the caller passed are compared; the component is identity. */
function compatible(plan, requested) {
  if ((plan.component ?? null) !== (requested.component ?? null)) return false;
  return Object.entries(requested).every(
    ([key, value]) => key === "component" || (key in plan && plan[key] === value),
  );
}

function validate(plan, origin) {
  if (plan === null || typeof plan !== "object" || plan.zin !== PLAN_FORMAT) {
    throw new PlanError(`${origin} is not a ${PLAN_FORMAT} file`);
  }
  try {
    const params = new Map(plan.actions.map((action) => [action.name, action.params.length]));
    plan.calls.forEach((call, index) => {
      if (!params.has(call.action)) {
        throw new PlanError(
          `the plan is malformed: call ${index} (${call.text}) uses the unknown action ${call.action}`,
        );
      }
      if (call.args.length !== params.get(call.action)) {
        throw new PlanError(
          `the plan is malformed: call ${index} (${call.text}) has ${call.args.length} arguments, ` +
            `but ${call.action} takes ${params.get(call.action)}`,
        );
      }
    });
    const states = plan.states.length;
    if (states === 0 || plan.parents.length !== states) {
      throw new PlanError("the plan is malformed: states and parents do not line up");
    }
    for (const test of plan.tests) {
      for (const [call, to] of test.steps) {
        if (!(call >= 0 && call < plan.calls.length && to >= -1 && to < states)) {
          throw new PlanError(`the plan is malformed: step [${call},${to}] is out of range`);
        }
      }
    }
  } catch (error) {
    if (error instanceof PlanError) throw error;
    throw new PlanError(`the plan is malformed: ${origin}: ${error.message}`);
  }
  return plan;
}

function readPlan(path) {
  let plan;
  try {
    plan = JSON.parse(readFileSync(path, "utf8"));
  } catch (error) {
    throw new PlanError(`cannot read plan ${path}: ${error.message}`);
  }
  return validate(plan, path);
}

/**
 * The plan for `spec`, following the runner contract in docs/plan-format.md:
 * `options.plan` when given, else a sibling `<name>.plan.json`, as long as it
 * is fresh and was derived with every planning option passed here; otherwise
 * the output of `zin plan` (`options.bin`, `$ZIN_BIN`, or `zin` on PATH).
 * Options left out are not compared. `spec` may itself be a plan file.
 */
function loadPlan(spec, options = {}) {
  const path = toPath(spec);
  const requested = {};
  const args = ["plan", path];
  for (const [name, key, flag] of OPTIONS) {
    const value = options[name];
    if (value === undefined || value === null) continue;
    if (key !== "cover" && key !== "component" && !(Number.isInteger(value) && value > 0)) {
      throw new PlanError(`${name} must be a positive integer, not ${value}`);
    }
    requested[key] = value;
    args.push(flag, String(value));
  }
  if (path.endsWith(".json")) {
    if (Object.keys(requested).length > 0 || options.plan !== undefined) {
      throw new PlanError("planning options need a .zin specification, not a plan file");
    }
    const plan = readPlan(path);
    const source = path.endsWith(".plan.json") ? `${path.slice(0, -".plan.json".length)}.zin` : null;
    if (source !== null && existsSync(source) && !fresh(plan, source)) {
      throw new PlanError(
        `${path} was derived from a different version of ${basename(source)}; ` +
          `regenerate it with: zin plan ${source} --out ${path}`,
      );
    }
    return plan;
  }
  if (!existsSync(path)) throw new PlanError(`cannot read specification ${path}`);
  const sibling = path.replace(/\.[^./\\]+$/, "") + ".plan.json";
  const explicit = options.plan !== undefined;
  const cached = explicit ? toPath(options.plan) : sibling;
  const regenerate = `zin ${[...args, "--out", cached].join(" ")}`;
  let reason = "";
  if (explicit || existsSync(cached)) {
    const plan = readPlan(cached);
    if (!fresh(plan, path)) reason = `${cached} was derived from a different version of ${basename(path)}`;
    else if (!compatible(plan, requested)) reason = `${cached} was derived with different planning options`;
    else return plan;
    if (explicit) throw new PlanError(`${reason}; regenerate it with: ${regenerate}`);
  }
  const prefix = reason ? `${reason}; ` : "";
  const binary = options.bin || process.env.ZIN_BIN || "zin";
  let output;
  try {
    output = execFileSync(binary, args, {
      encoding: "utf8",
      maxBuffer: 1024 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    if (error.code === "ENOENT") {
      throw new PlanError(
        `${prefix}the zin binary was not found. Install zin-spec, set ZIN_BIN, ` +
          `or regenerate the plan: ${regenerate}`,
      );
    }
    throw new PlanError(`zin plan ${path} failed:\n${String(error.stderr ?? error.message).trim()}`);
  }
  let plan;
  try {
    plan = JSON.parse(output);
  } catch (error) {
    throw new PlanError(`${binary} did not print a plan: ${error.message}`);
  }
  return validate(plan, binary);
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
      typeof observed === "object" &&
      Object.entries(observed).every(([key, item]) => key in expected && same(expected[key], item))
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
    // Counts steps attempted and matched, and fields observed, for reporters.
    this.progress = options.progress;
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
    if (typeof observed !== "object") {
      return { message: `observe() must return an object of state fields, not ${typeof observed}` };
    }
    const expected = this.plan.states[state];
    for (const [field, raw] of Object.entries(plain(observed))) {
      if (!(field in expected)) {
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
      if (progress) progress.attempted = index + 1;
      message = await this.step(this.text(step), async () => {
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

  async check(test, factory) {
    const steps = test.steps;
    const progress = this.progress;
    const found = await this.withAdapter(factory, (adapter) => this.run(adapter, steps, progress));
    if (found === null) return;
    const [failed, message] = found;
    const taken = steps.slice(0, failed);
    const digest = this.plan.fingerprint ?? "";
    const scenario =
      'scenario "Replay divergence"\n' +
      (taken.map((step) => `  ${this.text(step)}`).join("\n") || "  expect true");
    let report =
      `${this.plan.system}: implementation diverges from the specification ` +
      `in test "${test.name}" at step ${failed}\n${message}\nsteps taken:\n${this.listing(taken)}` +
      `\nspec fingerprint: ${digest}\n${scenario}`;
    if (failed > 0) {
      const short = this.shortest(steps, failed);
      if (short.length < taken.length) {
        let again;
        try {
          again = await this.withAdapter(factory, (adapter) => this.run(adapter, short));
        } catch (error) {
          report += `\nthe shorter replay raised: ${error?.message ?? error}`;
          throw new Divergence(report, digest, scenario);
        }
        report +=
          again !== null && again[0] === short.length && again[1] === message
            ? `\nshortest reproduction:\n${this.listing(short)}`
            : "\nthe shortest path to the same specification state does not reproduce the original mismatch";
      }
    }
    throw new Divergence(report, digest, scenario);
  }
}

/** Runs one plan test against fresh adapters from `factory`. */
async function runTest(plan, test, factory, options = {}) {
  await new Runner(plan, options).check(test, factory);
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
