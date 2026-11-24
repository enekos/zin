const { execFileSync, spawnSync } = require("node:child_process");
const { mkdtempSync, writeFileSync, rmSync } = require("node:fs");
const { join } = require("node:path");
const { tmpdir } = require("node:os");
const { expect, test } = require("@playwright/test");

test("browser CLI discovers controls and diagnoses bindings from a consuming project", () => {
  const dir = mkdtempSync(join(tmpdir(), "zin-doctor-"));
  const spec = join(__dirname, "../../fixtures/contract/latch.zin");
  const binary = process.env.ZIN_BIN || "zin";
  const opts = { cwd: join(__dirname, ".."), encoding: "utf8" };
  try {
    const url = "data:text/html," + encodeURIComponent('<button>Lock</button>');
    const source = execFileSync(binary, ["init", "playwright", spec, "--url", url], opts);
    expect(source).toContain('button("Lock")');
    expect(source).toContain("bind action unlock");
    const binding = join(dir, "binding.cjs");
    writeFileSync(binding, `const { bind, button } = require(${JSON.stringify(require.resolve("zin-verify/playwright"))});
      module.exports = bind({ url: ${JSON.stringify(url)}, actions: { lock: button("Missing") }, fields: { locked: () => false } });`);
    const result = spawnSync(binary, ["doctor", spec, "--binding", binding], opts);
    expect(result.status, result.stderr).toBe(1);
    expect(result.stdout).toContain("unbound action: unlock");
    expect(result.stdout).toContain('button "Missing" never matched');
    expect(result.stdout).toContain("constant projection");
  } finally { rmSync(dir, { recursive: true, force: true }); }
});
