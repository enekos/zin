"""Build the distributable artifacts and exercise them from clean consumers.

Each package is installed into an empty project that has only the package's
declared dependencies, so an import of an undeclared module fails here
before it fails for a user.
"""
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

LATCH = """system "Latch"

state
  locked is a boolean

initially
  locked is false

action lock
  requires locked is false
  locked becomes true

action unlock
  requires locked is true
  locked becomes false
"""


def run(*args, cwd, env=None):
    return subprocess.run([str(arg) for arg in args], cwd=cwd, env=env, check=True, text=True)


def crate(tmp, env):
    target = tmp / "target"
    env = {**env, "CARGO_TARGET_DIR": str(target)}
    run("cargo", "package", "--locked", "--allow-dirty", cwd=ROOT, env=env)
    package, = (target / "package").glob("*.crate")
    with tarfile.open(package) as archive:
        names = archive.getnames()
        assert all("node_modules" not in name and "/target/" not in name for name in names), names
        archive.extractall(tmp, filter="data")
    run("cargo", "install", "--locked", "--path", tmp / package.name.removesuffix(".crate"),
        "--root", tmp / "install", cwd=tmp, env=env)
    zin = tmp / "install/bin/zin"
    run(zin, "check", ROOT / "examples/account.zin", cwd=tmp)
    return zin


def python(tmp, env):
    # Build from a copy so setuptools output stays out of the checkout.
    source = tmp / "python-src"
    shutil.copytree(ROOT / "bindings/python", source,
                    ignore=shutil.ignore_patterns("__pycache__", ".pytest_cache", "*.egg-info", "build"))
    run(sys.executable, "-m", "venv", tmp / "venv", cwd=tmp)
    interpreter = tmp / "venv/bin/python"
    run(interpreter, "-m", "pip", "wheel", "--quiet", "--no-deps", "-w", tmp / "wheel", source, cwd=tmp)
    wheel, = (tmp / "wheel").glob("*.whl")
    run(interpreter, "-m", "pip", "install", "--quiet", "--no-deps", wheel, cwd=tmp)
    run(interpreter, "-c", PYTHON_CONSUMER, cwd=tmp, env=env)


def javascript(tmp, env):
    run("npm", "pack", "--silent", "--pack-destination", tmp, cwd=ROOT / "bindings/js")
    package, = tmp.glob("zin-verify-*.tgz")
    consumer = tmp / "node-consumer"
    consumer.mkdir()
    (consumer / "package.json").write_text(json.dumps({"private": True}))
    # Required peers are installed by npm; optional peers are not.
    run("npm", "install", "--silent", "--no-audit", "--no-fund", "--ignore-scripts", package,
        cwd=consumer)
    manifest = json.loads((consumer / "node_modules/zin-verify/package.json").read_text())
    entries = [("zin-verify" + name[1:], conditions)
               for name, conditions in manifest["exports"].items()]
    env = {**env, "ZIN_SMOKE_ENTRIES": json.dumps(entries)}
    run("node", "--input-type=commonjs", "-e", JS_CONSUMER, cwd=consumer, env=env)


PYTHON_CONSUMER = """
import os, zin

class Latch:
    def __init__(self, broken=False):
        self.locked = False
        self.broken = broken
    def lock(self):
        if self.locked and not self.broken:
            raise zin.Rejected()
        self.locked = True
    def unlock(self):
        if not self.locked:
            raise zin.Rejected()
        self.locked = False
    def observe(self):
        return {"locked": self.locked}

spec = os.environ["ZIN_SMOKE_SPEC"]
assert zin.check(spec, Latch)
try:
    zin.check(spec, lambda: Latch(broken=True))
except zin.Divergence as error:
    assert "lock" in str(error), error
else:
    raise AssertionError("the wheel accepted a broken latch")
"""

JS_CONSUMER = """
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const zin = require("zin-verify");

const adapter = broken => {
  let locked = false;
  return {
    actions: {
      lock: () => { if (locked && !broken) throw new zin.Rejected(); locked = true; },
      unlock: () => { if (!locked) throw new zin.Rejected(); locked = false; },
    },
    observe: () => ({ locked }),
  };
};

(async () => {
  const spec = process.env.ZIN_SMOKE_SPEC;
  await zin.check(spec, () => adapter(false));
  await assert.rejects(() => zin.check(spec, () => adapter(true)), zin.Divergence);
  // Every published entry point loads with only declared dependencies installed.
  const root = path.join("node_modules", "zin-verify");
  for (const [name, conditions] of JSON.parse(process.env.ZIN_SMOKE_ENTRIES)) {
    for (const [condition, files] of Object.entries(conditions)) {
      for (const file of Object.values(files)) {
        assert.ok(fs.existsSync(path.join(root, file)), `${name} ${condition}: missing ${file}`);
      }
    }
    if (conditions.require) require(name);
    if (conditions.import) await import(name);
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
"""


def main():
    with tempfile.TemporaryDirectory(prefix="zin-packages-") as work:
        tmp = Path(work)
        spec = tmp / "latch.zin"
        spec.write_text(LATCH)
        env = {**os.environ, "ZIN_SMOKE_SPEC": str(spec), "PIP_DISABLE_PIP_VERSION_CHECK": "1"}
        env["ZIN_BIN"] = str(crate(tmp, env))
        python(tmp, env)
        javascript(tmp, env)
    print("package smoke: crate, wheel and npm tarball pass from clean consumers")


if __name__ == "__main__":
    main()
