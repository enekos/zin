"""Run a Zin specification's derived test plan against a Python implementation.

    import zin

    @zin.verify("account.zin")
    class AccountConformance:
        rejects = (ValueError,)

        def setup(self):
            self.account = Account()

        def deposit(self, amount):
            self.account.deposit(amount)

        def observe(self):
            return {"balance": self.account.balance}

The decorated class becomes a ``unittest.TestCase`` with one test per plan
test, so pytest and unittest both collect and report it natively.
"""

from __future__ import annotations

import enum
import inspect
import json
import keyword
import os
import re
import shutil
import subprocess
import sys
import unittest
from pathlib import Path
from typing import Any, Callable, Iterable, Optional, Union

__all__ = [
    "Divergence",
    "PlanError",
    "Recorder",
    "Rejected",
    "check",
    "fingerprint",
    "load_plan",
    "run_test",
    "verify",
]
__version__ = "0.2.0"

PLAN_FORMAT = "plan/1"
# How many of the latest steps a report lists before eliding earlier ones.
LISTED_STEPS = 8
PathLike = Union[str, "os.PathLike[str]"]


class Rejected(Exception):
    """Raise from an action to say the implementation refused it."""


class Divergence(AssertionError):
    """A conformance failure with its original, pasteable Zin scenario."""

    def __init__(self, message: str, fingerprint: str = "", scenario: str = ""):
        super().__init__(message)
        self.fingerprint = fingerprint
        self.scenario = scenario


class PlanError(RuntimeError):
    """The test plan could not be loaded or derived."""


def fingerprint(source: bytes) -> str:
    """FNV-1a 64 of the specification bytes, as `zin plan` records it."""
    value = 0xCBF29CE484222325
    for byte in source:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return f"{value:016x}"


def _read_plan(path: Path) -> dict:
    try:
        plan = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise PlanError(f"cannot read plan {path}: {error}") from error
    if not isinstance(plan, dict) or plan.get("zin") != PLAN_FORMAT:
        raise PlanError(f"{path} is not a {PLAN_FORMAT} file")
    return plan


def load_plan(
    spec: PathLike,
    *,
    cover: Optional[str] = None,
    plan: Optional[PathLike] = None,
    bin: Optional[str] = None,
    max_steps: Optional[int] = None,
    step_budget: Optional[int] = None,
    pair_limit: Optional[int] = None,
    component: Optional[str] = None,
) -> dict:
    """Load an explicitly selected plan, a compatible sibling, or derive one."""
    spec = Path(spec)
    if spec.suffix == ".json":
        return _read_plan(spec)
    options = {"max_steps": max_steps, "step_budget": step_budget, "pair_limit": pair_limit}
    for key, value in options.items():
        if value is not None and (type(value) is not int or value < 1):
            raise PlanError(f"{key} must be a positive integer")
    try:
        wanted = fingerprint(spec.read_bytes())
    except OSError as error:
        raise PlanError(f"cannot read specification {spec}: {error}") from error
    args = ["plan", str(spec), "--cover", cover or "transitions"]
    for key, value in options.items():
        if value is not None:
            args.extend(["--" + key.replace("_", "-"), str(value)])
    if component is not None:
        args.extend(["--component", component])
    sibling = spec.with_suffix(".plan.json")
    cached = Path(plan) if plan is not None else sibling
    regenerate = "zin " + " ".join([*args, "--out", str(cached)])
    if plan is not None or cached.exists():
        loaded = _read_plan(cached)
        fresh = loaded.get("fingerprint") == wanted
        try:
            fresh = fresh and all(
                fingerprint((spec.parent / path).read_bytes()) == digest
                for path, digest in (loaded.get("sources") or {}).items()
            )
        except OSError:
            fresh = False
        if plan is not None and not fresh:
            raise PlanError(f"{cached} was derived from a different version of {spec.name}; regenerate with: {regenerate}")
        defaults = {"max_steps": 200, "step_budget": 2_000_000, "pair_limit": None}
        compatible = (cover is None and plan is not None) or loaded.get("cover") == (cover or "transitions")
        if component is not None or plan is None:
            compatible &= loaded.get("component") == component
        for key, value in options.items():
            if value is not None or plan is None:
                compatible &= loaded.get(key, defaults[key]) == (defaults[key] if value is None else value)
        if fresh and compatible:
            return loaded
        if plan is not None:
            raise PlanError(f"{cached} has different planning options; regenerate with: {regenerate}")
    binary = bin or os.environ.get("ZIN_BIN") or shutil.which("zin")
    hint = f"{sibling.name} is out of date; " if sibling.exists() else ""
    if not binary:
        raise PlanError(f"{hint}the zin binary was not found. Install zin-spec, set ZIN_BIN, or run: {regenerate}")
    try:
        done = subprocess.run([binary, *args], capture_output=True, text=True, encoding="utf-8", check=False)
    except OSError as error:
        raise PlanError(f"{hint}cannot run {binary}: {error}; regenerate with: {regenerate}") from error
    if done.returncode != 0:
        raise PlanError(f"zin plan {spec} failed:\n{done.stderr.strip()}")
    derived = json.loads(done.stdout)
    if derived.get("zin") != PLAN_FORMAT:
        raise PlanError(f"{binary} produced {derived.get('zin')!r}, but this package reads {PLAN_FORMAT}")
    return derived


def _plain(value: Any) -> Any:
    """An observed value in the plan's JSON vocabulary."""
    if isinstance(value, enum.Enum):
        return value.value if isinstance(value.value, str) else value.name
    if isinstance(value, dict):
        return {_key(key): _plain(item) for key, item in value.items()}
    return value


def _key(key: Any) -> str:
    key = _plain(key)
    if isinstance(key, bool):
        return "true" if key else "false"
    return str(key)


def _show(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, dict):
        return "{" + ", ".join(f"{key}: {_show(item)}" for key, item in value.items()) + "}"
    return str(value)


def _same(expected: Any, observed: Any) -> bool:
    if isinstance(expected, dict):
        return isinstance(observed, dict) and all(
            key in expected and _same(expected[key], item) for key, item in observed.items()
        )
    # bool is an int in Python; the specification keeps them apart.
    return type(expected) is type(observed) and expected == observed


def _python_name(name: str) -> str:
    return name + "_" if keyword.iskeyword(name) else name


class _Runner:
    def __init__(self, plan: dict):
        self.plan = plan
        self.params = {
            action["name"]: [param["name"] for param in action["params"]] for action in plan["actions"]
        }

    def text(self, step: list) -> str:
        call = self.plan["calls"][step[0]]["text"]
        return call if step[1] >= 0 else f"reject {call}"

    def compare(self, adapter: Any, state: int, after: str, progress=None) -> Optional[str]:
        observe = getattr(adapter, "observe", None)
        if observe is None:
            return None
        observed = observe()
        if observed is None:
            return None
        if not isinstance(observed, dict):
            return f"observe() must return a dict of state fields, not {type(observed).__name__}"
        expected = self.plan["states"][state]
        for field, actual in observed.items():
            if field not in expected:
                return f"observe() returned {field!r}, which is not a state field of the specification"
            if progress is not None:
                progress["fields"].add(field)
            actual = _plain(actual)
            if not _same(expected[field], actual):
                return (
                    f"{after}, {field} is {_show(actual)}, "
                    f"but the specification says {_show(expected[field])}"
                )
        return None

    def perform(self, adapter: Any, call: dict) -> bool:
        aliases = getattr(adapter, "zin_actions", {})
        handler = aliases.get(call["action"])
        method = getattr(adapter, handler or _python_name(call["action"]), None)
        if method is None:
            raise PlanError(
                f"{type(adapter).__name__} has no method for the action {call['action']!r}"
            )
        arguments = {
            _python_name(name): value for name, value in zip(self.params[call["action"]], call["args"])
        }
        rejects = (Rejected, *tuple(getattr(adapter, "rejects", ())))
        try:
            if handler:
                return method(dict(zip(self.params[call["action"]], call["args"]))) is not False
            return method(**arguments) is not False
        except rejects:
            return False

    def run(self, adapter: Any, steps: list, progress=None) -> Optional[tuple]:
        """The (step number, message) of the first divergence, if any."""
        message = self.compare(adapter, 0, "before any action", progress)
        if message:
            return 0, message
        state = 0
        for number, (call_id, to) in enumerate(steps, start=1):
            call = self.plan["calls"][call_id]
            if progress is not None:
                progress["attempted"] = number
            try:
                accepted = self.perform(adapter, call)
            except Exception as error:
                if hasattr(error, "add_note"):
                    error.add_note(
                        "zin: raised while running:\n" + self.listing(steps[:number])
                    )
                raise
            if to >= 0 and accepted:
                state = to
                message = self.compare(adapter, state, f"after {call['text']}", progress)
            elif to < 0 and not accepted:
                message = self.compare(adapter, state, f"after rejecting {call['text']}", progress)
            elif accepted:
                message = (
                    f"the implementation accepted {call['text']}, but the specification rejects it"
                )
            else:
                message = (
                    f"the implementation rejected {call['text']}, but the specification allows it"
                )
            if message:
                return number, message
            if progress is not None:
                progress["matched"] = number
        return None

    def listing(self, steps: list) -> str:
        skipped = max(0, len(steps) - LISTED_STEPS)
        lines = [f"  ... {skipped} earlier steps"] if skipped else []
        lines += [
            f"  {number}. {self.text(step)}"
            for number, step in enumerate(steps[skipped:], start=skipped + 1)
        ]
        return "\n".join(lines)

    def shortest(self, steps: list, failed: int) -> list:
        state = 0
        for _, to in steps[: failed - 1]:
            if to >= 0:
                state = to
        path = []
        while self.plan["parents"][state] is not None:
            parent, call = self.plan["parents"][state]
            path.append([call, state])
            state = parent
        path.reverse()
        return path + [steps[failed - 1]]

    def check(self, test: dict, factory: Callable[[], Any], progress=None) -> None:
        steps = test["steps"]
        def fresh(sequence: list, tracking=None):
            adapter = factory()
            try:
                return self.run(adapter, sequence, tracking)
            finally:
                teardown = getattr(adapter, "teardown", None)
                if teardown is not None:
                    teardown()

        found = fresh(steps, progress)
        if found is None:
            return
        failed, message = found
        taken = steps[:failed]
        report = (
            f"{self.plan['system']}: implementation diverges from the specification "
            f"in test \"{test['name']}\" at step {failed}\n{message}\nsteps taken:\n{self.listing(taken)}"
        )
        scenario = 'scenario "Replay divergence"\n' + ("\n".join("  " + self.text(step) for step in taken) or "  expect true")
        digest = self.plan.get("fingerprint", "")
        report += f"\nspec fingerprint: {digest}\n{scenario}"
        if failed > 0:
            short = self.shortest(steps, failed)
            if len(short) < len(taken):
                try:
                    again = fresh(short)
                except Exception as error:
                    again = None
                    report += f"\nshorter replay failed: {error}"
                if again == (len(short), message):
                    report += "\nshortest reproduction:\n" + self.listing(short[: again[0]])
                else:
                    report += (
                        "\nthe shortest path to the same specification state does not reproduce "
                        "the original mismatch"
                    )
        raise Divergence(report, digest, scenario)


def run_test(plan: dict, test: dict, factory: Callable[[], Any]) -> None:
    """Runs one plan test against fresh adapters from `factory`."""
    _Runner(plan).check(test, factory)


def _caller_directory(depth: int) -> Path:
    frame = sys._getframe(depth)
    filename = frame.f_globals.get("__file__")
    return Path(filename).resolve().parent if filename else Path.cwd()


def check(
    spec: PathLike,
    factory: Callable[[], Any],
    *,
    cover: Optional[str] = None,
    plan: Optional[PathLike] = None,
    max_steps: Optional[int] = None,
    step_budget: Optional[int] = None,
    pair_limit: Optional[int] = None,
    component: Optional[str] = None,
) -> dict:
    """Runs every plan test; raises `Divergence` on the first failure.

    Relative paths are resolved against the calling file. Returns the plan's
    summary.
    """
    base = _caller_directory(2)
    loaded = load_plan(base / spec, cover=cover, plan=None if plan is None else base / plan,
                           max_steps=max_steps, step_budget=step_budget, pair_limit=pair_limit, component=component)
    runner = _Runner(loaded)
    for test in loaded["tests"]:
        runner.check(test, factory)
    return loaded["summary"]


def _method_name(index: int, name: str, width: int) -> str:
    slug = re.sub(r"[^0-9a-zA-Z]+", "_", name).strip("_").lower()
    return f"test_{index:0{width}d}_{slug}"


def verify(
    spec: PathLike,
    *,
    cover: Optional[str] = None,
    plan: Optional[PathLike] = None,
    max_steps: Optional[int] = None,
    step_budget: Optional[int] = None,
    pair_limit: Optional[int] = None,
    component: Optional[str] = None,
) -> Callable[[type], type]:
    """Class decorator: turn an adapter class into a `unittest.TestCase`.

    The adapter class has one method per action (arguments are passed by
    parameter name), and may define `setup()`, `teardown()`, `observe()` and
    `rejects`. Relative paths are resolved against the file defining the class.
    """

    def decorate(adapter_class: type) -> type:
        try:
            base = Path(inspect.getfile(adapter_class)).resolve().parent
        except (TypeError, OSError):
            base = Path.cwd()
        loaded = load_plan(base / spec, cover=cover, plan=None if plan is None else base / plan,
                           max_steps=max_steps, step_budget=step_budget, pair_limit=pair_limit, component=component)
        runner = _Runner(loaded)
        namespace: dict = {
            "__module__": adapter_class.__module__,
            "__qualname__": adapter_class.__qualname__,
            "__doc__": adapter_class.__doc__,
            "adapter": adapter_class,
            "plan": loaded,
        }
        width = len(str(len(loaded["tests"])))

        def make(test: dict) -> Callable[[unittest.TestCase], None]:
            def method(self: unittest.TestCase) -> None:
                def factory() -> Any:
                    adapter = adapter_class()
                    setup = getattr(adapter, "setup", None)
                    if setup is not None:
                        try:
                            setup()
                        except BaseException:
                            teardown = getattr(adapter, "teardown", None)
                            if teardown is not None:
                                teardown()
                            raise
                    return adapter

                runner.check(test, factory, getattr(self, "_zin_progress", None))

            method._zin_plan_test = test
            method.__doc__ = f"{loaded['system']}: {test['name']}"
            return method

        for index, test in enumerate(loaded["tests"], start=1):
            namespace[_method_name(index, test["name"], width)] = make(test)
        return type(adapter_class.__name__, (unittest.TestCase,), namespace)

    return decorate


class Recorder:
    """Writes the JSON-lines event log that `zin trace <spec> <log>` checks.

    Wrap production or integration-test calls::

        recorder = zin.Recorder("events.jsonl")
        recorder.event("deposit", {"amount": 5}, accepted=True, state={"balance": 5})
    """

    def __init__(self, target: Union[PathLike, Any]):
        self._owned = not hasattr(target, "write")
        self._file = open(target, "a", encoding="utf-8") if self._owned else target

    def _write(self, record: dict) -> None:
        self._file.write(json.dumps(record, separators=(",", ":")) + "\n")
        self._file.flush()

    def event(
        self,
        action: str,
        args: Union[dict, Iterable[Any], None] = None,
        *,
        accepted: bool = True,
        state: Optional[dict] = None,
    ) -> None:
        record: dict = {
            "action": action,
            "args": _plain(args) if isinstance(args, dict) else [_plain(arg) for arg in args or ()],
            "outcome": "accepted" if accepted else "rejected",
        }
        if state is not None:
            record["state"] = _plain(state)
        self._write(record)

    def reset(self) -> None:
        """Marks the start of a new run from the initial state."""
        self._write({"reset": True})

    def close(self) -> None:
        if self._owned:
            self._file.close()

    def __enter__(self) -> "Recorder":
        return self

    def __exit__(self, *_: Any) -> None:
        self.close()
