"""Build the distributable artifacts and exercise them from a clean consumer."""
import json
import os
import shutil
import subprocess
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def run(*args, cwd, env=None):
    return subprocess.run(args, cwd=cwd, env=env, check=True, text=True)


def smoke(tmp):
    target = tmp / "target"
    env = {**os.environ, "CARGO_TARGET_DIR": str(target)}
    run("cargo", "package", "--locked", "--allow-dirty", cwd=ROOT, env=env)
    crate, = (target / "package").glob("*.crate")
    with tarfile.open(crate) as archive:
        names = archive.getnames()
        assert all("node_modules" not in name and "/target/" not in name for name in names)
        archive.extractall(tmp, filter="data")
    run("cargo", "install", "--locked", "--path", tmp / crate.stem, "--root", tmp / "install",
        cwd=tmp, env=env)
    zin = tmp / "install/bin/zin"
    run(zin, "check", ROOT / "examples/account.zin", cwd=tmp)
    env["ZIN_BIN"] = str(zin)
    source = ROOT / "bindings/fixtures/latch.zin"
    env["ZIN_SMOKE_SPEC"] = str(source)
    # Build the wheel from its own directory, keeping setuptools output out of the checkout.
    shutil.copytree(ROOT / "bindings/python", tmp / "python-src",
                    ignore=shutil.ignore_patterns("__pycache__", ".pytest_cache", "*.egg-info", "build"))
    run("python3", "-m", "venv", tmp / "venv", cwd=tmp)
    python = tmp / "venv/bin/python"
    run(python, "-m", "pip", "wheel", "--no-deps", "-w", tmp, tmp / "python-src", cwd=tmp)
    wheel, = tmp.glob("*.whl")
    run(python, "-m", "pip", "install", "--no-deps", wheel, cwd=tmp)
    run(python, "-c", PYTHON_CONSUMER, cwd=tmp, env=env)

    run("npm", "pack", "--pack-destination", tmp, cwd=ROOT / "bindings/js")
    package, = tmp.glob("*.tgz")
    consumer = tmp / "node-consumer"
    consumer.mkdir()
    (consumer / "package.json").write_text(json.dumps({"private": True}))
    run("npm", "install", "--no-audit", "--no-fund", "--ignore-scripts", package, cwd=consumer)
    run("node", "--input-type=commonjs", "-e", JS_CONSUMER, cwd=consumer, env=env)


PYTHON_CONSUMER = """
import os, zin
class Latch:
    def __init__(self): self.locked = False
    def lock(self):
        accepted = not self.locked
        self.locked = True
        return accepted
    def unlock(self):
        accepted = self.locked
        self.locked = False
        return accepted
    def observe(self): return {"locked": self.locked}

class Faulty(Latch):
    def lock(self):
        super().lock()
        return True

source = os.environ["ZIN_SMOKE_SPEC"]
assert zin.check(source, Latch)["tests"] > 0
try: zin.check(source, Faulty)
except zin.Divergence as error: assert "lock" in str(error)
else: raise AssertionError("wheel accepted the broken latch")
"""

JS_CONSUMER = """
const assert = require("node:assert/strict");
const zin = require("zin-verify");
const adapter = broken => {
  let locked = false;
  const set = value => {
    const accepted = locked !== value;
    locked = value;
    return accepted || broken;
  };
  return { actions: { lock: () => set(true), unlock: () => set(false) }, observe: () => ({ locked }) };
};
(async () => {
  const spec = process.env.ZIN_SMOKE_SPEC;
  await zin.check(spec, () => adapter(false));
  await assert.rejects(() => zin.check(spec, () => adapter(true)), zin.Divergence);
  await import("zin-verify/vitest-reporter");
})().catch(error => { console.error(error); process.exitCode = 1; });
"""

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="zin-packages-") as work:
        smoke(Path(work))
