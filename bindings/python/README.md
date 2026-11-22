# zin-verify (Python)

Run a [Zin](https://github.com/enekos/zin) specification as conformance tests
in pytest or unittest. No dependencies.

```python
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
```

The plan comes from the `zin` binary (`ZIN_BIN` or `PATH`) or from a committed
`account.plan.json`. See
[conformance testing](https://github.com/enekos/zin/blob/main/docs/conformance.md).

API: `verify`, `check`, `load_plan`, `run_test`, `Recorder`, `Rejected`,
`Divergence`, `PlanError`.

## Model coverage and replay

Run `pytest --zin-coverage` after installing the binding. The native pytest
plugin reports unique (model state, call) operations attempted, planned and
eligible, failed operations, and the fields actually compared by `observe`.
Name filtering and early failures reduce the executed count. Shorter diagnostic
replays do not count twice. With `cover="pairs"`, the report also counts
adjacent calls. From a source checkout the plugin can be loaded explicitly
with `python -m pytest -p zin.pytest_plugin --zin-coverage`.

Select a component of a composition with `component="Client"`. Pass `max_steps` for a soft tour target, `step_budget` for a hard total, or
`cover="pairs", pair_limit=100` for a deterministic sample. Sibling plans
must match the requested settings and fingerprints of the entry and its included
files. A direct plan JSON path is a self-contained artifact. A specification
and an explicit `plan=` path also check source freshness.

A `Divergence` carries `fingerprint` and `scenario`. Paste the scenario into
the spec and rerun the same adapter to exercise the original failing prefix.
Action names that conflict with adapter hooks or Python identifiers can be
mapped explicitly, e.g. `zin_actions = {"setup": "perform_setup"}`. Mapped
methods receive a dictionary with the original parameter names.
