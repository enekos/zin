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

## Planning options and committed plans

`verify`, `check` and `load_plan` take `cover`, `max_steps`, `step_budget`,
`pair_limit` and `component`, the options of `zin plan`. A committed sibling
`account.plan.json` (or an explicit `plan=`) is used when it is fresh for the
specification and every included file, and was derived with each option you
pass. A sibling must also have been derived with zin's default for every
option you leave out, so a plan with weaker coverage is never picked up
silently; an explicit plan is your opt-in to its options. A path to a `.json` plan replays
that plan, after checking it against `account.zin` beside it. See
[the runner contract](../../docs/plan-format.md#what-a-binding-must-do).

A `Divergence` carries `fingerprint` and `scenario`: paste the scenario into
the specification to keep the failing prefix as a worked example.

## Model coverage

`pytest --zin-coverage` reports, per suite, the unique (model state, call)
operations attempted out of those planned and eligible, the operations that
failed, and the fields `observe` actually compared. Name filters and early
failures reduce the executed count; replays that shorten a failure are not
counted. With `cover="pairs"` it also counts adjacent calls. From a source
checkout, load the plugin with `python -m pytest -p zin.pytest_plugin --zin-coverage`.
