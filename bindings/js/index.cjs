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
const { basename } = require("node:path");
const { fileURLToPath } = require("node:url");

const PLAN_FORMAT = "plan/1";
// How many of the latest steps a report lists before eliding earlier ones.
const LISTED_STEPS = 8;

/** Throw from an action to say the implementation refused it. */
class Rejected extends Error {
  constructor(message = "rejected") {
    super(message);
    this.name = "Rejected";
  }
}

/** The implementation does not behave as the specification says. */
class Divergence extends Error {
  constructor(message) {
    super(message);
    this.name = "Divergence";
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
  if (options.plan !== undefined) {
    const planPath = toPath(options.plan);
    const plan = readPlan(planPath);
    if (plan.fingerprint !== wanted) {
      throw new PlanError(
        `${planPath} was derived from a different version of ${basename(path)}; ` +
          `regenerate it with: zin plan ${path} --out ${planPath}`,
      );
    }
    return plan;
  }
  const sibling = path.replace(/\.[^./\\]+$/, "") + ".plan.json";
  let stale = false;
  if (existsSync(sibling)) {
    const plan = readPlan(sibling);
    if (plan.fingerprint === wanted && plan.cover === cover) return plan;
    stale = true;
  }
  const binary = options.bin || process.env.ZIN_BIN || "zin";
  let output;
  try {
    output = execFileSync(binary, ["plan", path, "--cover", cover], {
      encoding: "utf8",
      maxBuffer: 1024 * 1024 * 1024,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    if (error.code === "ENOENT") {
      const hint = stale ? `${basename(sibling)} is out of date and ` : "";
      throw new PlanError(
        `${hint}the zin binary was not found. Install it (cargo install zin), set ZIN_BIN, ` +
          `or commit a plan: zin plan ${path} --out ${sibling}`,
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

function plain(value) {
  if (value instanceof Map) {
    return Object.fromEntries([...value].map(([key, item]) => [String(key), plain(item)]));
  }
  if (value !== null && typeof value === "object" && !Array.isArray(value)) {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, plain(item)]));
  }
  return value;
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

class Runner {
  constructor(plan) {
    this.plan = plan;
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

  async compare(adapter, state, after) {
    if (typeof adapter.observe !== "function") return null;
    const observed = await adapter.observe();
    if (observed === null || observed === undefined) return null;
    if (typeof observed !== "object") {
      return `observe() must return an object of state fields, not ${typeof observed}`;
    }
    const expected = this.plan.states[state];
    for (const [field, raw] of Object.entries(plain(observed))) {
      if (!(field in expected)) {
        return `observe() returned '${field}', which is not a state field of the specification`;
      }
      if (!same(expected[field], raw)) {
        return `${after}, ${field} is ${show(raw)}, but the specification says ${show(expected[field])}`;
      }
    }
    return null;
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
  async run(adapter, steps) {
    let message = await this.compare(adapter, 0, "before any action");
    if (message) return [0, message];
    let state = 0;
    for (const [index, [callId, to]] of steps.entries()) {
      const call = this.plan.calls[callId];
      const accepted = await this.perform(adapter, call);
      if (to >= 0 && accepted) {
        state = to;
        message = await this.compare(adapter, state, `after ${call.text}`);
      } else if (to < 0 && !accepted) {
        message = await this.compare(adapter, state, `after rejecting ${call.text}`);
      } else if (accepted) {
        message = `the implementation accepted ${call.text}, but the specification rejects it`;
      } else {
        message = `the implementation rejected ${call.text}, but the specification allows it`;
      }
      if (message) return [index + 1, message];
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
    const found = await this.withAdapter(factory, (adapter) => this.run(adapter, steps));
    if (found === null) return;
    const [failed, message] = found;
    const taken = steps.slice(0, failed);
    let report =
      `${this.plan.system}: implementation diverges from the specification ` +
      `in test "${test.name}" at step ${failed}\n${message}\nsteps taken:\n${this.listing(taken)}`;
    if (failed > 0) {
      const short = this.shortest(steps, failed);
      if (short.length < taken.length) {
        const again = await this.withAdapter(factory, (adapter) => this.run(adapter, short));
        report += again
          ? `\nshortest reproduction:\n${this.listing(short.slice(0, again[0]))}`
          : "\nthe shortest path to the same specification state does not reproduce this: " +
            "the implementation keeps state the specification does not model";
      }
    }
    throw new Divergence(report);
  }
}

/** Runs one plan test against fresh adapters from `factory`. */
async function runTest(plan, test, factory) {
  await new Runner(plan).check(test, factory);
}

/** Runs every plan test; rejects with `Divergence` on the first failure. */
async function check(spec, factory, options = {}) {
  const plan = loadPlan(spec, options);
  const runner = new Runner(plan);
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
  const runner = new Runner(plan);
  describe(options.name ?? `${plan.system} conforms to ${basename(toPath(spec))}`, () => {
    for (const planTest of plan.tests) {
      test(planTest.name, () => runner.check(planTest, factory));
    }
  });
  return plan;
}

/** Writes the JSON-lines event log that `zin trace <spec> <log>` checks. */
class Recorder {
  constructor(target) {
    this.write =
      typeof target === "function" ? target : (line) => appendFileSync(toPath(target), line);
  }

  event(action, args = [], { accepted = true, state } = {}) {
    const record = { action, args: plain(args), outcome: accepted ? "accepted" : "rejected" };
    if (state !== undefined) record.state = plain(state);
    this.write(`${JSON.stringify(record)}\n`);
  }

  /** Marks the start of a new run from the initial state. */
  reset() {
    this.write('{"reset":true}\n');
  }
}

module.exports = { PLAN_FORMAT, Rejected, Divergence, PlanError, fingerprint, loadPlan, runTest, check, verify, Recorder };
