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
__version__ = "0.3.0"

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


# The planning options a caller may pass, with their `zin plan` flags.
OPTIONS = {
    "cover": "--cover",
    "max_steps": "--max-steps",
    "step_budget": "--step-budget",
    "pair_limit": "--pair-limit",
    "component": "--component",
}


def _current_fingerprint(spec: Path, sources: dict) -> Optional[str]:
    """The fingerprint `zin plan` would record now, or None if a file is unreadable.

    With included files, the entry file's hash and each source's current hash,
    sorted by path, are hashed together (see docs/plan-format.md).
    """
    try:
        entry = fingerprint(spec.read_bytes())
        if not sources:
            return entry
        text = entry + "\n" + "".join(
            f"{path}\t{fingerprint((spec.parent / path).read_bytes())}\n" for path in sorted(sources)
        )
    except OSError:
        return None
    return fingerprint(text.encode("utf-8"))


def _fresh(plan: dict, spec: Path) -> bool:
    return _current_fingerprint(spec, plan.get("sources") or {}) == plan.get("fingerprint")


def _compatible(plan: dict, options: dict) -> bool:
    """Only options the caller passed are compared; the component is identity."""
    if plan.get("component") != options.get("component"):
        return False
    return all(key in plan and plan[key] == value for key, value in options.items() if key != "component")


def _validate(plan: Any, origin: Any) -> dict:
    if not isinstance(plan, dict) or plan.get("zin") != PLAN_FORMAT:
        raise PlanError(f"{origin} is not a {PLAN_FORMAT} file")
    try:
        params = {action["name"]: len(action["params"]) for action in plan["actions"]}
        for index, call in enumerate(plan["calls"]):
            if call["action"] not in params:
                raise PlanError(
                    f"the plan is malformed: call {index} ({call['text']}) uses the unknown action {call['action']}"
                )
            if len(call["args"]) != params[call["action"]]:
                raise PlanError(
                    f"the plan is malformed: call {index} ({call['text']}) has {len(call['args'])} arguments, "
                    f"but {call['action']} takes {params[call['action']]}"
                )
        states = len(plan["states"])
        if states == 0 or len(plan["parents"]) != states:
            raise PlanError("the plan is malformed: states and parents do not line up")
        for test in plan["tests"]:
            for call, to in test["steps"]:
                if not (0 <= call < len(plan["calls"]) and -1 <= to < states):
                    raise PlanError(f"the plan is malformed: step {[call, to]} is out of range")
    except (KeyError, TypeError, ValueError) as error:
        raise PlanError(f"the plan is malformed: {origin}: {error!r}") from error
    return plan


def _read_plan(path: Path) -> dict:
    try:
        plan = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as error:
        raise PlanError(f"cannot read plan {path}: {error}") from error
    return _validate(plan, path)


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
    """The plan for `spec`, following the runner contract in docs/plan-format.md.

    Uses `plan` when given, else a sibling ``<name>.plan.json``, as long as it
    is fresh and was derived with every option passed here; otherwise runs
    ``zin plan`` (`bin`, ``$ZIN_BIN`` or ``zin`` on PATH). Options left out are
    not compared. `spec` may itself be a plan file.
    """
    spec = Path(spec)
    options = {
        key: value
        for key, value in (
            ("cover", cover),
            ("max_steps", max_steps),
            ("step_budget", step_budget),
            ("pair_limit", pair_limit),
            ("component", component),
        )
        if value is not None
    }
    for key in ("max_steps", "step_budget", "pair_limit"):
        value = options.get(key)
        if value is not None and (type(value) is not int or value < 1):
            raise PlanError(f"{key} must be a positive integer")
    if spec.suffix == ".json":
        if options or plan is not None:
            raise PlanError("planning options need a .zin specification, not a plan file")
        loaded = _read_plan(spec)
        source = spec.with_name(spec.name[: -len(".plan.json")] + ".zin") if spec.name.endswith(".plan.json") else None
        if source is not None and source.exists() and not _fresh(loaded, source):
            raise PlanError(
                f"{spec} was derived from a different version of {source.name}; "
                f"regenerate it with: zin plan {source} --out {spec}"
            )
        return loaded
    if not spec.is_file():
        raise PlanError(f"cannot read specification {spec}")
    args = ["plan", str(spec)]
    for key, value in options.items():
        args += [OPTIONS[key], str(value)]
    sibling = spec.with_suffix(".plan.json")
    cached = Path(plan) if plan is not None else sibling
    regenerate = "zin " + " ".join([*args, "--out", str(cached)])
    reason = ""
    if plan is not None or cached.exists():
        loaded = _read_plan(cached)
        if not _fresh(loaded, spec):
            reason = f"{cached} was derived from a different version of {spec.name}"
        elif not _compatible(loaded, options):
            reason = f"{cached} was derived with different planning options"
        else:
            return loaded
        if plan is not None:
            raise PlanError(f"{reason}; regenerate it with: {regenerate}")
    binary = bin or os.environ.get("ZIN_BIN") or shutil.which("zin")
    prefix = f"{reason}; " if reason else ""
    missing = (
        f"{prefix}the zin binary was not found. Install zin-spec, set ZIN_BIN, "
        f"or regenerate the plan: {regenerate}"
    )
    if not binary:
        raise PlanError(missing)
    try:
        done = subprocess.run([binary, *args], capture_output=True, text=True, encoding="utf-8", check=False)
    except FileNotFoundError as error:
        raise PlanError(missing) from error
    except OSError as error:
        raise PlanError(f"{prefix}cannot run {binary}: {error}") from error
    if done.returncode != 0:
        raise PlanError(f"zin plan {spec} failed:\n{done.stderr.strip()}")
    try:
        derived = json.loads(done.stdout)
    except ValueError as error:
        raise PlanError(f"{binary} did not print a plan: {error}") from error
    return _validate(derived, binary)


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

    def compare(self, adapter: Any, state: int, after: str, progress: Optional[dict] = None) -> Optional[str]:
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
        method = getattr(adapter, _python_name(call["action"]), None)
        if method is None:
            raise PlanError(
                f"{type(adapter).__name__} has no method for the action {call['action']!r}"
            )
        arguments = {
            _python_name(name): value for name, value in zip(self.params[call["action"]], call["args"])
        }
        rejects = (Rejected, *tuple(getattr(adapter, "rejects", ())))
        try:
            return method(**arguments) is not False
        except rejects:
            return False

    def run(self, adapter: Any, steps: list, progress: Optional[dict] = None) -> Optional[tuple]:
        """The (step number, message) of the first divergence, if any.

        `progress` counts the steps attempted and matched, and the fields observed.
        """
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

    def replay(self, factory: Callable[[], Any], steps: list, progress: Optional[dict] = None) -> Optional[tuple]:
        """Runs `steps` on a fresh adapter, then tears it down."""
        adapter = factory()
        try:
            return self.run(adapter, steps, progress)
        finally:
            teardown = getattr(adapter, "teardown", None)
            if teardown is not None:
                teardown()

    def check(self, test: dict, factory: Callable[[], Any], progress: Optional[dict] = None) -> None:
        steps = test["steps"]
        found = self.replay(factory, steps, progress)
        if found is None:
            return
        failed, message = found
        taken = steps[:failed]
        digest = self.plan.get("fingerprint", "")
        scenario = 'scenario "Replay divergence"\n' + (
            "\n".join("  " + self.text(step) for step in taken) or "  expect true"
        )
        report = (
            f"{self.plan['system']}: implementation diverges from the specification "
            f"in test \"{test['name']}\" at step {failed}\n{message}\nsteps taken:\n{self.listing(taken)}"
            f"\nspec fingerprint: {digest}\n{scenario}"
        )
        if failed > 0:
            short = self.shortest(steps, failed)
            if len(short) < len(taken):
                try:
                    again = self.replay(factory, short)
                except Exception as error:  # noqa: BLE001 - the original mismatch is the report
                    report += f"\nthe shorter replay raised: {error}"
                else:
                    if again == (len(short), message):
                        report += "\nshortest reproduction:\n" + self.listing(short)
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
    plan: Optional[PathLike] = None,
    **options: Any,
) -> dict:
    """Runs every plan test; raises `Divergence` on the first failure.

    `options` are the planning options of `load_plan`. Relative paths are
    resolved against the calling file. Returns the plan's summary.
    """
    base = _caller_directory(2)
    loaded = load_plan(base / spec, plan=None if plan is None else base / plan, **options)
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
    plan: Optional[PathLike] = None,
    **options: Any,
) -> Callable[[type], type]:
    """Class decorator: turn an adapter class into a `unittest.TestCase`.

    The adapter class has one method per action (arguments are passed by
    parameter name), and may define `setup()`, `teardown()`, `observe()` and
    `rejects`. `options` are the planning options of `load_plan`. Relative
    paths are resolved against the file defining the class.
    """

    def decorate(adapter_class: type) -> type:
        try:
            base = Path(inspect.getfile(adapter_class)).resolve().parent
        except (TypeError, OSError):
            base = Path.cwd()
        loaded = load_plan(base / spec, plan=None if plan is None else base / plan, **options)
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
                        setup()
                    return adapter

                # The pytest plugin sets this to record replay progress.
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
