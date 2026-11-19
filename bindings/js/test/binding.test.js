import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { copyFileSync, mkdtempSync, readFileSync, writeFileSync, appendFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { Divergence, PlanError, Recorder, Rejected, check, fingerprint, loadPlan, runTest, verify } from "../index.js";
import { Account, adapter } from "./bank.js";

const ACCOUNT = new URL("../../../examples/account.zin", import.meta.url);
const BROKEN = new URL("../../../examples/broken/lost-update.zin", import.meta.url);
const ZIN = process.env.ZIN_BIN || "zin";

const diverges = (pattern) => (error) => {
  assert.ok(error instanceof Divergence, String(error));
  assert.match(error.message, pattern);
  return true;
};

test("fingerprint matches the checker", () => {
  assert.equal(loadPlan(ACCOUNT).fingerprint, fingerprint(readFileSync(ACCOUNT)));
  assert.equal(fingerprint(new Uint8Array()), "cbf29ce484222325");
  assert.equal(fingerprint(new TextEncoder().encode("a")), "af63dc4c8601ec8c");
});

test("a correct implementation conforms", async () => {
  const summary = await check(ACCOUNT, adapter());
  assert.equal(summary.transitions, 182);
  await check(ACCOUNT, adapter(null, false));
  await check(ACCOUNT, adapter(), { cover: "states" });
});

test("an off-by-one is caught with or without observe", async () => {
  for (const observable of [true, false]) {
    await assert.rejects(
      check(ACCOUNT, adapter("cap", observable)),
      diverges(/accepted deposit\(\d\), but the specification rejects it[\s\S]*steps taken:/),
    );
  }
});

test("a missing guard is caught", async () => {
  await assert.rejects(check(ACCOUNT, adapter("close")), diverges(/the implementation accepted close/));
});

test("a wrong state is caught by observe", async () => {
  await assert.rejects(
    check(ACCOUNT, adapter("bonus")),
    diverges(/after reopen, balance is 1, but the specification says 0/),
  );
});

test("unexpected errors are errors, not rejections", async () => {
  const crashing = () => ({
    ...adapter()(),
    actions: new Proxy({}, { get: () => () => { throw new TypeError("boom"); } }),
  });
  await assert.rejects(check(ACCOUNT, crashing), TypeError);
});

test("returning false and throwing Rejected both reject; maps and teardown work", async () => {
  const plan = {
    zin: "plan/1",
    system: "Lights",
    actions: [{ name: "on", params: [{ name: "room" }] }, { name: "off", params: [{ name: "room" }] }],
    states: [{ lit: { hall: false } }, { lit: { hall: true } }],
    parents: [null, [0, 0]],
    calls: [
      { action: "on", args: ["hall"], text: "on(hall)" },
      { action: "off", args: ["hall"], text: "off(hall)" },
    ],
    tests: [{ name: "tour 1", kind: "tour", steps: [[1, -1], [0, 1], [0, -1], [1, 0]] }],
  };
  let torn = 0;
  const factory = async () => {
    const lit = new Map([["hall", false]]);
    return {
      actions: {
        on: ({ room }) => (lit.get(room) ? false : void lit.set(room, true)),
        off: ({ room }) => {
          if (!lit.get(room)) throw new Rejected();
          lit.set(room, false);
        },
      },
      observe: () => ({ lit }),
      teardown: () => void torn++,
    };
  };
  await runTest(plan, plan.tests[0], factory);
  assert.equal(torn, 1);

  const wrong = async () => ({ ...(await factory()), observe: () => ({ lit: { hall: 0 } }) });
  await assert.rejects(runTest(plan, plan.tests[0], wrong), diverges(/lit is \{hall: 0\}, but the specification says \{hall: false\}/));
});

test("verify registers one test per plan test with any describe/test pair", async () => {
  const registered = [];
  const plan = verify(ACCOUNT, adapter("cap"), {
    describe: (name, body) => { registered.push(name); body(); },
    test: (name, body) => registered.push([name, body]),
  });
  assert.equal(registered[0], "Bank account conforms to account.zin");
  assert.equal(registered.length - 1, plan.tests.length);
  const outcomes = await Promise.allSettled(registered.slice(1).map(([, body]) => body()));
  assert.ok(outcomes.some((outcome) => outcome.status === "rejected"));
  assert.ok(outcomes.some((outcome) => outcome.status === "fulfilled"));
});

test("a committed plan is used without the binary", () => {
  const directory = mkdtempSync(join(tmpdir(), "zin-"));
  const spec = join(directory, "account.zin");
  copyFileSync(ACCOUNT, spec);
  writeFileSync(join(directory, "account.plan.json"), JSON.stringify(loadPlan(ACCOUNT)));
  assert.equal(loadPlan(spec, { bin: "/nonexistent/zin" }).system, "Bank account");

  appendFileSync(spec, "\n# edited\n");
  assert.throws(() => loadPlan(spec, { bin: "/nonexistent/zin" }), /out of date/);
  assert.throws(() => loadPlan(spec, { plan: join(directory, "account.plan.json") }), /different version/);
});

test("an invalid specification fails with the checker's message", () => {
  assert.throws(() => loadPlan(BROKEN), (error) => error instanceof PlanError && /counterexample/.test(error.message));
});

test("Recorder output is accepted by zin trace", async () => {
  const directory = mkdtempSync(join(tmpdir(), "zin-"));
  const log = join(directory, "events.jsonl");
  const account = new Account();
  const recorder = new Recorder(log);
  account.deposit(5);
  recorder.event("deposit", { amount: 5 }, { state: { balance: account.balance } });
  recorder.event("close", [], { accepted: false });
  await account.withdraw(5);
  recorder.event("withdraw", [5], { state: { status: account.status } });
  recorder.reset();
  recorder.event("close");
  assert.match(execFileSync(ZIN, ["trace", ACCOUNT.pathname, log], { encoding: "utf8" }), /4 events in 2 run\(s\)/);

  new Recorder(log).event("deposit", [5]);
  const bad = spawnSync(ZIN, ["trace", ACCOUNT.pathname, log], { encoding: "utf8" });
  assert.equal(bad.status, 1);
  assert.match(bad.stderr, /trace line 6/);
});

// A light that takes a moment to change, as a rendered interface does.
const LIGHT = {
  zin: "plan/1",
  system: "Light",
  actions: [{ name: "toggle", params: [] }, { name: "smash", params: [] }],
  states: [{ lit: false }, { lit: true }],
  parents: [null, [0, 0]],
  calls: [
    { action: "toggle", args: [], text: "toggle" },
    { action: "smash", args: [], text: "smash" },
  ],
  tests: [{ name: "tour 1", kind: "tour", steps: [[0, 1], [1, -1], [0, 0]] }],
};

const slowLight = (delay, extra = {}) => () => {
  let lit = false;
  return {
    actions: {
      toggle: () => void setTimeout(() => { lit = !lit; }, delay),
      smash: () => false,
    },
    observe: () => ({ lit }),
    ...extra,
  };
};

test("observeTimeout waits for an implementation that settles later", async () => {
  await assert.rejects(runTest(LIGHT, LIGHT.tests[0], slowLight(30)), diverges(/after toggle, lit is false/));
  await runTest(LIGHT, LIGHT.tests[0], slowLight(30), { observeTimeout: 2000, observeInterval: 5 });
  await assert.rejects(
    runTest(LIGHT, LIGHT.tests[0], slowLight(10_000), { observeTimeout: 60, observeInterval: 5 }),
    diverges(/lit is false, but the specification says true \(waited 60 ms\)/),
  );
});

test("a malformed observation fails at once instead of waiting", async () => {
  const started = Date.now();
  await assert.rejects(
    runTest(LIGHT, LIGHT.tests[0], slowLight(0, { observe: () => ({ glow: 1 }) }), { observeTimeout: 5000 }),
    diverges(/'glow', which is not a state field/),
  );
  assert.ok(Date.now() - started < 2000);
});

test("settle runs after every action and step wraps every step", async () => {
  let settled = 0;
  const steps = [];
  const settling = slowLight(20, { settle: () => new Promise((resolve) => setTimeout(() => resolve(void settled++), 40)) });
  await runTest(LIGHT, LIGHT.tests[0], settling, {
    step: async (text, body) => {
      steps.push(text);
      return body();
    },
  });
  assert.equal(settled, 3);
  assert.deepEqual(steps, ["toggle", "reject smash", "toggle"]);
});

test("maxSteps shortens the generated tours", () => {
  const longest = (plan) => Math.max(...plan.tests.map((item) => item.steps.length));
  assert.equal(longest(loadPlan(ACCOUNT)), 200);
  assert.ok(longest(loadPlan(ACCOUNT, { maxSteps: 40 })) < 60);
  assert.throws(() => loadPlan(ACCOUNT, { maxSteps: 0 }), PlanError);
  assert.throws(() => loadPlan(ACCOUNT, { maxSteps: 40, bin: "/nonexistent/zin" }), /--max-steps 40/);
});
