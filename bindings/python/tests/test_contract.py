"""The runner contract shared by every binding: bindings/fixtures/contract."""

import copy
import dataclasses
import enum
import json
from pathlib import Path

import pytest

import zin

ROOT = Path(__file__).resolve().parents[3]
CONTRACT = ROOT / "bindings" / "fixtures" / "contract"
CASES = json.loads((CONTRACT / "cases.json").read_text())
MISSING_BINARY = str(CONTRACT / "no-such-zin")


@pytest.mark.parametrize("case", CASES["load"], ids=lambda case: case["name"])
def test_load(case):
    plan = case.get("plan")
    options = dict(
        case.get("options", {}),
        bin=MISSING_BINARY,
        plan=None if plan is None else CONTRACT / plan,
    )
    expect = case["expect"]
    if "errors" in expect:
        with pytest.raises(zin.PlanError) as caught:
            zin.load_plan(CONTRACT / case["spec"], **options)
        for fragment in expect["errors"]:
            assert fragment in str(caught.value)
    else:
        loaded = zin.load_plan(CONTRACT / case["spec"], **options)
        assert loaded["fingerprint"] == expect["fingerprint"]
        assert loaded["cover"] == expect["cover"]


class Scripted:
    """The contract's table-driven adapter (see the contract README)."""

    def __init__(self, script, instance):
        self.script = script
        self.instance = instance
        self.calls = 0
        self.state = copy.deepcopy(script["initial"])
        if script["observe"]:
            self.observe = lambda: dict(self.state)
        for action in script["rules"]:
            setattr(self, action, lambda action=action: self.call(action))

    def call(self, action):
        self.calls += 1
        for override in self.script.get("overrides", []):
            if (
                override.get("instance", self.instance) == self.instance
                and override.get("call", self.calls) == self.calls
                and override.get("action", action) == action
            ):
                if override["outcome"] == "raise":
                    raise RuntimeError(override["message"])
                return override["outcome"] == "accept"
        rule = self.script["rules"][action]
        if any(self.state[key] != value for key, value in rule["requires"].items()):
            return False
        self.state.update(rule["set"])
        return True


@pytest.mark.parametrize("case", CASES["replay"], ids=lambda case: case["name"])
def test_replay(case):
    plan = zin.load_plan(CONTRACT / case["plan"])
    script = {**CASES["adapter"], **case["adapter"]}
    made = []

    def factory():
        made.append(Scripted(script, len(made) + 1))
        return made[-1]

    expect = case["expect"]
    for index in case["tests"]:
        test = plan["tests"][index]
        if expect.get("passes"):
            zin.run_test(plan, test, factory)
        elif "raises" in expect:
            with pytest.raises(RuntimeError, match=expect["raises"]):
                zin.run_test(plan, test, factory)
        else:
            with pytest.raises(zin.Divergence) as caught:
                zin.run_test(plan, test, factory)
            assert str(caught.value) == expect["report"]
    if "factories" in expect:
        assert len(made) == expect["factories"]


class Token(enum.Enum):
    OPEN = "open"
    CLOSED = "closed"
    ANN = "ann"
    BEN = "ben"
    IDLE = "idle"
    BUSY = "busy"


def native(value):
    if isinstance(value, dict) and "$enum" in value:
        return Token(value["$enum"])
    if isinstance(value, dict) and "$set" in value:
        return {native(item) for item in value["$set"]}
    if isinstance(value, dict) and "$record" in value:
        members = {key: native(item) for key, item in value["$record"].items()}
        return dataclasses.make_dataclass("Record", list(members))(**members)
    if isinstance(value, dict) and "$map" in value:
        return {native(key): native(item) for key, item in value["$map"]}
    if isinstance(value, dict):
        return {key: native(item) for key, item in value.items()}
    return value


def matches(expected, observed):
    plan = {
        "system": "Observation",
        "actions": [],
        "calls": [],
        "states": [expected],
        "parents": [None],
        "tests": [{"name": "t", "kind": "tour", "steps": []}],
    }

    class Observer:
        def observe(self):
            return observed

    try:
        zin.run_test(plan, plan["tests"][0], Observer)
    except zin.Divergence:
        return False
    return True


@pytest.mark.parametrize(
    "case",
    json.loads((CONTRACT / "observations.json").read_text())
    + json.loads((CONTRACT / "native-observations.json").read_text()),
    ids=lambda case: case["name"],
)
def test_observation(case):
    assert matches(case["expected"], native(case["observed"])) is case["matches"]


def test_plans_have_every_key_the_schema_requires():
    schema = json.loads((ROOT / "docs" / "plan.schema.json").read_text())
    for name in ("latch.plan.json", "composed/main.plan.json"):
        plan = zin.load_plan(CONTRACT / name)
        assert set(schema["required"]) <= set(plan), name
        assert set(schema["properties"]["summary"]["required"]) <= set(plan["summary"]), name
