# Integrating Zin with a real app

The easiest integration has three pieces: a `.zin` behavioral contract, your application, and a small conformance test that exercises the same journeys through the application. Zin checks the model; your test runner checks the implementation.

This example uses Python because it needs no packages, but the same workflow works with any language or framework.

## 1. Put the contract beside the app

Start with the account specification from [`examples/account.zin`](../examples/account.zin). The important part is one action contract and one review journey:

```zin
action withdraw(amount is an integer from 1 to 5)
  requires status is open
  requires amount is at most balance
  balance becomes balance - amount

scenario "Deposit, withdraw, and close"
  deposit with amount = 5
  expect balance is 5
  withdraw with amount = 5
  close
  expect status is closed
```

Run the model checker whenever this file changes:

```sh
zin check examples/account.zin
```

## 2. Implement the same actions in the app

A small `account.py` might look like this:

```python
class Account:
    def __init__(self):
        self.balance = 0
        self.status = "open"

    def deposit(self, amount):
        self._require_open()
        if not 1 <= amount <= 5 or self.balance + amount > 20:
            raise ValueError("deposit is not enabled")
        self.balance += amount

    def withdraw(self, amount):
        self._require_open()
        if not 1 <= amount <= 5 or amount > self.balance:
            raise ValueError("withdraw is not enabled")
        self.balance -= amount

    def close(self):
        self._require_open()
        if self.balance != 0:
            raise ValueError("close is not enabled")
        self.status = "closed"

    def reopen(self):
        if self.status != "closed":
            raise ValueError("reopen is not enabled")
        self.status = "open"

    def _require_open(self):
        if self.status != "open":
            raise ValueError("account is not open")
```

The methods correspond to Zin actions, and their guards correspond to `requires` conditions. An HTTP handler, UI command, or message consumer can call these methods without changing the behavioral contract.

## 3. Turn the journey into a conformance test

Create `test_account.py`:

```python
import unittest

from account import Account


class AccountConformanceTest(unittest.TestCase):
    def test_deposit_withdraw_and_close(self):
        account = Account()

        account.deposit(5)
        self.assertEqual(account.balance, 5)

        account.withdraw(5)
        account.close()
        self.assertEqual(account.status, "closed")

    def test_overdraw_is_not_enabled(self):
        account = Account()

        with self.assertRaises(ValueError):
            account.withdraw(1)


if __name__ == "__main__":
    unittest.main()
```

The first test is the executable implementation version of the Zin scenario. The second checks an important disabled path that a successful journey does not cover.

Run both layers of verification:

```sh
zin check examples/account.zin
python -m unittest test_account.py
```

## 4. Review behavior in pull requests

Generate the human-readable review alongside the code change:

```sh
zin report examples/account.zin --out account-review.md
```

The report gives reviewers the promises, action contracts, reachable-state Mermaid chart, and scenario sequence diagrams. They can review system behavior before reading implementation details.

A useful pull-request routine is:

1. Change the `.zin` contract, implementation, and conformance test together.
2. Run `zin check` and the app's normal test suite in CI.
3. Attach or publish the generated Markdown report.
4. Add a conformance test for each new scenario and each important disabled action.

Zin does **not** execute or prove `account.py`. The model checker proves the specification; the conformance tests provide evidence that the app follows it. Keeping the two beside each other makes intentional behavior changes easy to review and accidental drift easier to catch.
