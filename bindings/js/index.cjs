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
// The plan keys that shape coverage; `component` is identity, not an option.
const PLANNING_OPTIONS = ["cover", "max_steps", "step_budget", "pair_limit"];

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

/**
 * Passed options must match; the component is identity. A sibling picked
 * automatically must also match the plan's recorded defaults for every option
 * left out, so weaker coverage is never chosen silently. An explicit plan is
 * the opt-in to whatever it was derived with.
 */
function compatible(plan, requested, explicit) {
  if ((plan.component ?? null) !== (requested.component ?? null)) return false;
  const defaults = plan.defaults !== null && typeof plan.defaults === "object" ? plan.defaults : {};
  if ((requested.restore !== undefined || !explicit) && (plan.zin === "plan/2") !== !!requested.restore) return false;
  const observed = plan.observe;
  if (requested.observe !== undefined) {
    if (!Array.isArray(observed) || observed.length !== requested.observe.length ||
        !requested.observe.every(field => observed.includes(field))) return false;
  } else if (!explicit && observed !== undefined) return false;
  return PLANNING_OPTIONS.every((key) => {
    if (key in requested) return key in plan && plan[key] === requested[key];
    if (explicit) return true;
    return key in defaults && key in plan && plan[key] === defaults[key];
  });
}

function validate(plan, origin) {
  if (plan === null || typeof plan !== "object" || ![PLAN_FORMAT, "plan/2"].includes(plan.zin)) {
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
      if (test.from !== undefined && (plan.zin !== "plan/2" || !Number.isInteger(test.from) || test.from < 0 || test.from >= states)) {
        throw new PlanError("the plan is malformed: invalid restore state");
      }
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
 * A sibling must also use the default for every option left out; an explicit
 * `options.plan` is not compared on those. `spec` may itself be a plan file.
 */
function loadPlan(spec, options = {}) {
  const path = toPath(spec);
  const requested = {};
  const args = ["plan", path];
  if (options.restore !== undefined) {
    requested.restore = options.restore;
    if (options.restore) args.push("--restore");
  }
  if (options.observe !== undefined) {
    if (!Array.isArray(options.observe) || options.observe.some(field => typeof field !== "string" || !field || field.includes(","))) {
      throw new PlanError("observe must be an array of field names");
    }
    requested.observe = [...new Set(options.observe)];
    args.push("--observe", requested.observe.join(","));
  }
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
    else if (!compatible(plan, requested, explicit)) reason = `${cached} was derived with different planning options`;
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
    this.location = options.location;
    this.monitor = options.monitor;
    this.onState = options.onState;
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
    const projection = this.plan.observe ?? [];
    for (const field of projection) {
      if (!Object.hasOwn(observed ?? {}, field)) {
        return { message: `observe() omitted the planned field '${field}'` };
      }
    }
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
      if (!same(expected[field], raw) || (projection.includes(field) && !same(raw, expected[field]))) {
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
    if (typeof adapter.observe !== "function") {
      return this.plan.observe?.length ? "the plan requires an observe() function" : null;
    }
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
  async run(adapter, steps, progress, from = 0) {
    if (from !== 0) {
      if (typeof adapter.restore !== "function") throw new PlanError("this plan needs adapter.restore(state)");
      await adapter.restore(structuredClone(this.plan.states[from]));
      if (adapter.settle) await adapter.settle();
    }
    let message = await this.compare(adapter, from, from ? "after restoring state" : "before any action", progress);
    if (message) return [0, message];
    const monitor = this.monitor && await this.monitor();
    try {
      return await this.runSteps(adapter, steps, progress, from, monitor);
    } finally {
      if (monitor) await monitor.dispose();
    }
  }

  async runSteps(adapter, steps, progress, state, monitor) {
    const seen = new Set();
    const visit = async id => {
      if (this.onState && !seen.has(id)) {
        seen.add(id);
        await this.onState(structuredClone(this.plan.states[id]), id);
      }
    };
    let message = await monitor?.allow([this.plan.states[state]]);
    if (message) return [0, message];
    await visit(state);
    for (let index = 0; index < steps.length; index++) {
      const step = steps[index];
      const [callId, to] = step;
      const call = this.plan.calls[callId];
      if (progress) progress.attempted = index + 1;
      if (monitor) {
        const allowed = [this.plan.states[state]];
        if (to >= 0) allowed.push(this.plan.states[to]);
        message = await monitor.allow(allowed);
        if (message) return [index + 1, message];
      }
      if (to < 0 && adapter.refusals) {
        let end = index + 1;
        while (end < steps.length && steps[end][1] < 0) end++;
        const calls = steps.slice(index, end).map(([id]) => this.plan.calls[id]);
        const refused = await adapter.refusals(calls);
        if (refused === true) {
          message = await this.step(calls.map(call => `reject ${call.text}`).join(", "), async () => {
            if (adapter.settle) await adapter.settle();
            return this.compare(adapter, state, "after checking refusals", progress);
          });
          if (progress) progress.attempted = end;
          if (message) return [end, message];
          if (progress) progress.matched = end;
          index = end - 1;
          continue;
        }
      }
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
      }, this.location?.(call.action));
      if (!message && monitor) message = await monitor.allow([this.plan.states[state]]);
      if (message) return [index + 1, message];
      if (this.onState && to >= 0) await visit(state);
      if (progress) progress.matched = index + 1;
    }
    if (monitor) {
      message = await monitor.allow([this.plan.states[state]]);
      if (message) return [steps.length, message];
    }
    return null;
  }

  pathTo(state) {
    const path = [];
    while (this.plan.parents[state] !== null) {
      const [parent, call] = this.plan.parents[state];
      path.push([call, state]);
      state = parent;
    }
    path.reverse();
    return path;
  }

  shortest(steps, failed, state) {
    for (const [, to] of steps.slice(0, failed - 1)) {
      if (to >= 0) state = to;
    }
    return [...this.pathTo(state), steps[failed - 1]];
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
    const found = await this.withAdapter(factory, (adapter) => this.run(adapter, steps, progress, test.from));
    if (found === null) return;
    const [failed, message] = found;
    const taken = steps.slice(0, failed);
    const digest = this.plan.fingerprint ?? "";
    const scenario =
      'scenario "Replay divergence"\n' +
      ([...this.pathTo(test.from ?? 0), ...taken].map((step) => `  ${this.text(step)}`).join("\n") || "  expect true");
    let report =
      `${this.plan.system}: implementation diverges from the specification ` +
      `in test "${test.name}" at step ${failed}\n${message}\nsteps taken:\n${this.listing(taken)}` +
      `\nspec fingerprint: ${digest}\n${scenario}`;
    if (test.from) report += `\nstarted by restoring state ${test.from}: ${show(this.plan.states[test.from])}`;
    if (failed > 0) {
      const location = this.location?.(this.plan.calls[steps[failed - 1][0]].action);
      if (location) report += `\nspecification: ${location.file}:${location.line}:${location.column}`;
      const short = this.shortest(steps, failed, test.from ?? 0);
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
