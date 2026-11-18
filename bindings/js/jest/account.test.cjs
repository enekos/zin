const { join } = require("node:path");
const { verify } = require("zin-verify");
const { adapter } = require("../test/bank.cjs");

// Each derived case is a native Jest test, as in Python's unittest/pytest binding.
let created = 0;
let disposed = 0;
const plan = verify(join(__dirname, "../../../examples/account.zin"), async () => {
  created++;
  return { ...adapter()(), teardown: async () => { disposed++; } };
});

afterAll(() => {
  expect(created).toBe(plan.tests.length);
  expect(disposed).toBe(created);
});
