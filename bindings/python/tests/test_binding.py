import io
import json
import os
import shutil
import subprocess
import unittest
from pathlib import Path

import pytest

import zin

from bank import Account

EXAMPLES = Path(__file__).resolve().parents[3] / "examples"
ACCOUNT = EXAMPLES / "account.zin"
ZIN = os.environ.get("ZIN_BIN") or shutil.which("zin")


class Adapter:
    rejects = (ValueError,)
    bug = None
    observable = True

    def setup(self):
        self.account = Account(self.bug)

    def deposit(self, amount):
        self.account.deposit(amount)

    def withdraw(self, amount):
        self.account.withdraw(amount)

    def close(self):
        self.account.close()

    def reopen(self):
        self.account.reopen()

    def observe(self):
        if not self.observable:
            return None
        return {"balance": self.account.balance, "status": self.account.status}


def factory(bug=None, observable=True):
    def make():
        adapter = Adapter()
        adapter.bug = bug
        adapter.observable = observable
        adapter.setup()
        return adapter

    return make


def test_fingerprint_matches_the_checker():
    plan = zin.load_plan(ACCOUNT)
    assert plan["fingerprint"] == zin.fingerprint(ACCOUNT.read_bytes())
    assert zin.fingerprint(b"") == "cbf29ce484222325"
    assert zin.fingerprint(b"a") == "af63dc4c8601ec8c"


def test_correct_implementation_conforms():
    summary = zin.check(ACCOUNT, factory())
    assert summary["transitions"] == 182
    zin.check(ACCOUNT, factory(observable=False))
    zin.check(ACCOUNT, factory(), cover="states")


@pytest.mark.parametrize("observable", [True, False])
def test_off_by_one_is_caught(observable):
    with pytest.raises(zin.Divergence) as caught:
        zin.check(ACCOUNT, factory("cap", observable))
    message = str(caught.value)
    assert "accepted deposit(" in message
    assert "but the specification rejects it" in message
    assert "steps taken:" in message


def test_missing_guard_is_caught_with_a_short_reproduction():
    with pytest.raises(zin.Divergence) as caught:
        zin.check(ACCOUNT, factory("close"))
    message = str(caught.value)
    assert "the implementation accepted close" in message
    if "shortest reproduction:" in message:
        assert len(message.split("shortest reproduction:\n")[1].splitlines()) <= 2


def test_wrong_state_is_caught_by_observe():
    with pytest.raises(zin.Divergence) as caught:
        zin.check(ACCOUNT, factory("bonus"))
    assert "after reopen, balance is 1, but the specification says 0" in str(caught.value)


def test_hidden_state_bug_needs_observe():
    # The fee bug only changes the balance, so with observe() it is found at once.
    with pytest.raises(zin.Divergence) as caught:
        zin.check(ACCOUNT, factory("fee"))
    assert "but the specification says" in str(caught.value)
    # Without observe() it still surfaces, later, as a wrongly allowed or refused action.
    with pytest.raises(zin.Divergence) as caught:
        zin.check(ACCOUNT, factory("fee", observable=False))
    assert "the implementation accepted" in str(caught.value) or "the implementation rejected" in str(caught.value)


def test_unexpected_exceptions_are_errors_not_rejections():
    class Crashing(Adapter):
        def deposit(self, amount):
            raise KeyError("boom")

    def make():
        adapter = Crashing()
        adapter.setup()
        return adapter

    with pytest.raises(KeyError):
        zin.check(ACCOUNT, make)


def test_returning_false_and_raising_rejected_both_reject():
    class Polite:
        def setup(self):
            self.on = False

        def toggle(self):
            self.on = not self.on

        def switch_on(self):
            if self.on:
                return False
            self.on = True

        def switch_off(self):
            if not self.on:
                raise zin.Rejected()
            self.on = False

        def observe(self):
            return {"on": self.on}

    plan = {
        "zin": "plan/1",
        "system": "Light",
        "actions": [{"name": "switch_on", "params": []}, {"name": "switch_off", "params": []}],
        "states": [{"on": False}, {"on": True}],
        "parents": [None, [0, 0]],
        "calls": [
            {"action": "switch_on", "args": [], "text": "switch_on"},
            {"action": "switch_off", "args": [], "text": "switch_off"},
        ],
        "tests": [{"name": "tour 1", "kind": "tour", "steps": [[1, -1], [0, 1], [0, -1], [1, 0]]}],
    }

    def make():
        adapter = Polite()
        adapter.setup()
        return adapter

    zin.run_test(plan, plan["tests"][0], make)


def test_bool_and_int_are_not_confused():
    plan = {
        "system": "Flag",
        "actions": [],
        "states": [{"on": False}],
        "parents": [None],
        "calls": [],
        "tests": [{"name": "t", "kind": "tour", "steps": []}],
    }

    class Zero:
        def observe(self):
            return {"on": 0}

    with pytest.raises(zin.Divergence, match="on is 0, but the specification says false"):
        zin.run_test(plan, plan["tests"][0], Zero)


def test_decorator_builds_a_test_case_per_plan_test():
    @zin.verify(ACCOUNT)
    class Generated(Adapter):
        pass

    assert issubclass(Generated, unittest.TestCase)
    names = [name for name in dir(Generated) if name.startswith("test_")]
    assert len(names) == len(Generated.plan["tests"])
    assert any("scenario" in name for name in names)
    result = unittest.TestResult()
    unittest.defaultTestLoader.loadTestsFromTestCase(Generated).run(result)
    assert result.wasSuccessful(), result.failures + result.errors

    @zin.verify(ACCOUNT)
    class Buggy(Adapter):
        bug = "cap"

    result = unittest.TestResult()
    unittest.defaultTestLoader.loadTestsFromTestCase(Buggy).run(result)
    assert result.failures and not result.errors
    assert "but the specification rejects it" in result.failures[0][1]


def test_committed_plan_is_used_without_the_binary(tmp_path, monkeypatch):
    spec = tmp_path / "account.zin"
    shutil.copy(ACCOUNT, spec)
    (tmp_path / "account.plan.json").write_text(json.dumps(zin.load_plan(ACCOUNT)))
    monkeypatch.setenv("ZIN_BIN", "")
    monkeypatch.setenv("PATH", "")
    assert zin.load_plan(spec)["system"] == "Bank account"

    spec.write_text(spec.read_text() + "\n# edited\n")
    with pytest.raises(zin.PlanError, match="different version.*the zin binary was not found"):
        zin.load_plan(spec)
    with pytest.raises(zin.PlanError, match="different version"):
        zin.load_plan(spec, plan=tmp_path / "account.plan.json")


def test_invalid_specification_fails_with_the_checker_message():
    with pytest.raises(zin.PlanError, match="counterexample"):
        zin.load_plan(EXAMPLES / "broken" / "lost-update.zin")


@pytest.mark.skipif(not ZIN, reason="needs the zin binary")
def test_recorder_output_is_accepted_by_zin_trace(tmp_path):
    log = tmp_path / "events.jsonl"
    account = Account()
    with zin.Recorder(log) as recorder:
        account.deposit(5)
        recorder.event("deposit", {"amount": 5}, state={"balance": account.balance})
        recorder.event("close", accepted=False)
        account.withdraw(5)
        recorder.event("withdraw", [5], state={"status": account.status})
        recorder.reset()
        recorder.event("close")
    done = subprocess.run([ZIN, "trace", str(ACCOUNT), str(log)], capture_output=True, text=True)
    assert done.returncode == 0, done.stderr
    assert "4 events in 2 run(s)" in done.stdout

    buffer = io.StringIO()
    zin.Recorder(buffer).event("deposit", [5], state={"balance": 4})
    log.write_text(buffer.getvalue())
    done = subprocess.run([ZIN, "trace", str(ACCOUNT), str(log)], capture_output=True, text=True)
    assert done.returncode == 1
    assert "balance" in done.stderr


def test_pytest_reporter_tracks_filtered_and_failed_tests(tmp_path):
    import sys

    plan = EXAMPLES.parent / "tests/fixtures/v0.2/switch.plan.json"
    test = tmp_path / "test_switch.py"
    test.write_text(
        f"""import zin
@zin.verify({str(plan)!r})
class Switch:
    def setup(self): self.active = False
    def observe(self): return {{"active": self.active}}
    def start(self): self.active = True
    def stop(self): self.active = False
"""
    )
    env = dict(os.environ, PYTEST_DISABLE_PLUGIN_AUTOLOAD="1", PYTHONPATH=str(EXAMPLES.parent / "bindings/python"))
    command = [sys.executable, "-m", "pytest", "-q", "-p", "zin.pytest_plugin", "--zin-coverage", str(test), "-k"]
    for selector, code, row in [
        ("start_only", 0, "operations 1/3 planned, 0 failed"),
        ("cycle", 1, "operations 2/3 planned, 1 failed"),
    ]:
        done = subprocess.run([*command, selector], text=True, capture_output=True, env=env)
        assert done.returncode == code and row in done.stdout, done.stdout + done.stderr
        assert "observed: active" in done.stdout


@pytest.mark.skipif(not ZIN, reason="needs the zin binary")
def test_a_component_plan_is_cached_under_its_own_scope(tmp_path):
    path = tmp_path / "protocol.zin"
    shutil.copy(EXAMPLES / "request-reply.zin", path)
    component = zin.load_plan(path, component="Client")
    assert component["component"] == "Client"
    path.with_suffix(".plan.json").write_text(json.dumps(component))
    assert zin.load_plan(path, component="Client", bin="/no/zin") == component
    with pytest.raises(zin.PlanError, match="different planning options"):
        zin.load_plan(path, bin="/no/zin")
