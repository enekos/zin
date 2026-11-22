"""Native pytest reporting for Zin suites, enabled with --zin-coverage."""
import hashlib
import json

import pytest


def _obligations(steps, count):
    state, previous = 0, None
    for call, to in steps[:count]:
        op = f"{state}/{call}"
        yield op, f"{previous}/{call}" if previous is not None else None
        previous = op
        if to >= 0:
            state = to


def _model(plan):
    ops, pairs = set(), set()
    with_pairs = plan["cover"] == "pairs"
    for case in plan["tests"]:
        for op, pair in _obligations(case["steps"], len(case["steps"])):
            ops.add(op)
            if pair and with_pairs:
                pairs.add(pair)
    summary = plan["summary"]
    return {
        "id": hashlib.sha256(json.dumps(plan, sort_keys=True).encode()).hexdigest(),
        "name": plan["system"], "planned": len(ops),
        "eligible": summary.get("refusals", 0) + summary["transitions"] if "refusals" in summary else None,
        "planned_pairs": len(pairs) if with_pairs else None, "eligible_pairs": summary.get("eligible_pairs"),
    }


def pytest_addoption(parser):
    parser.getgroup("zin").addoption("--zin-coverage", action="store_true", help="show executed Zin model coverage")


def pytest_configure(config):
    if config.getoption("--zin-coverage"):
        config.pluginmanager.register(_Reporter(), "zin-model-coverage")


class _Reporter:
    def __init__(self):
        self.models = {}
        self.groups = {}

    def add(self, suite, model):
        return self.groups.setdefault((suite, model["id"]), {
            "model": model, "executed": set(), "failed": set(), "fields": set(),
            "pairs": set(), "failed_pairs": set(),
        })

    @pytest.hookimpl(tryfirst=True)
    def pytest_collection_modifyitems(self, items):
        for item in items:
            case = getattr(getattr(item, "obj", None), "_zin_plan_test", None)
            if case is None:
                continue
            cls = item.cls
            if cls not in self.models:
                self.models[cls] = _model(cls.plan)
            model = self.models[cls]
            suite = item.parent.nodeid
            self.add(suite, model)
            item._zin_record = (suite, model, case)

    @pytest.hookimpl(tryfirst=True)
    def pytest_runtest_call(self, item):
        if hasattr(item, "_zin_record"):
            item.instance._zin_progress = {"attempted": 0, "matched": 0, "fields": set()}

    @pytest.hookimpl(hookwrapper=True)
    def pytest_runtest_makereport(self, item, call):
        report = (yield).get_result()
        if not hasattr(item, "_zin_record"):
            return
        if call.when != "call" and not (call.when == "setup" and report.outcome != "passed"):
            return
        suite, model, case = item._zin_record
        progress = getattr(item.instance, "_zin_progress", {"attempted": 0, "matched": 0, "fields": set()})
        attempted = list(_obligations(case["steps"], progress["attempted"]))
        record = {"suite": suite, "model": model, "executed": sorted({op for op, _ in attempted}),
                  "pairs": sorted({pair for _, pair in attempted if pair and model["planned_pairs"] is not None}),
                  "fields": sorted(progress["fields"])}
        if progress["attempted"] > progress["matched"]:
            record["failed"], record["failed_pair"] = attempted[-1]
        report.user_properties.append(("zin_coverage", record))

    def pytest_runtest_logreport(self, report):
        for name, record in report.user_properties:
            if name != "zin_coverage":
                continue
            group = self.add(record["suite"], record["model"])
            for field in ("executed", "pairs", "fields"):
                group[field].update(record[field])
            if record.get("failed"):
                group["failed"].add(record["failed"])
            if record.get("failed_pair"):
                group["failed_pairs"].add(record["failed_pair"])

    def pytest_terminal_summary(self, terminalreporter):
        for (suite, _), group in sorted(self.groups.items()):
            model = group["model"]
            scope = "" if model["eligible"] is None else f", {model['eligible']} eligible"
            pairs = "" if model["planned_pairs"] is None else (
                f"; pairs {len(group['pairs'])}/{model['planned_pairs']} planned, "
                f"{model['eligible_pairs']} eligible, {len(group['failed_pairs'])} failed"
            )
            fields = ", ".join(sorted(group["fields"])) or "(none)"
            terminalreporter.write_line(
                f"Zin {suite}: operations {len(group['executed'])}/{model['planned']} planned"
                f"{scope}, {len(group['failed'])} failed{pairs}; observed: {fields}"
            )
