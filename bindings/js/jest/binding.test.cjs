const { join } = require("node:path");
const { check, Divergence, Rejected, runTest } = require("zin-verify");
const { adapter } = require("../test/bank.cjs");
const spec = join(__dirname, "../../../examples/account.zin");

test.each(["cap", "close", "bonus"])("reports a %s bug and cleans up every adapter", async (bug) => {
  let created = 0;
  let disposed = 0;
  await expect(check(spec, async () => {
    created++;
    return { ...adapter(bug)(), teardown: async () => { disposed++; } };
  })).rejects.toThrow(Divergence);
  expect(created).toBeGreaterThan(0);
  expect(disposed).toBe(created);
});

test("unexpected errors propagate and still trigger teardown", async () => {
  const teardown = jest.fn();
  const error = new TypeError("implementation crashed");
  await expect(check(spec, () => ({
    actions: new Proxy({}, { get: () => () => { throw error; } }), teardown,
  }))).rejects.toBe(error);
  expect(teardown).toHaveBeenCalledTimes(1);
});

test("async rejections and Map observations work inside Jest's VM", async () => {
  const plan = {
    system: "Lights",
    actions: [{ name: "on", params: [] }],
    states: [{ lit: { hall: false } }],
    parents: [null],
    calls: [{ action: "on", args: [], text: "on" }],
  };
  await runTest(plan, { name: "refusal", steps: [[0, -1]] }, async () => ({
    actions: { on: async () => { throw new Rejected(); } },
    observe: async () => ({ lit: new Map([["hall", false]]) }),
  }));
});
