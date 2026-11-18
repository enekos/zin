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
