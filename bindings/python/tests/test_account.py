"""What a user writes: one adapter class, and the specification does the rest."""

import zin

from bank import Account

SPEC = "../../../examples/account.zin"


@zin.verify(SPEC)
class AccountConformance:
    rejects = (ValueError,)

    def setup(self):
        self.account = Account()

    def deposit(self, amount):
        self.account.deposit(amount)

    def withdraw(self, amount):
        self.account.withdraw(amount)

    def close(self):
        self.account.close()

    def reopen(self):
        self.account.reopen()

    def observe(self):
        return {"balance": self.account.balance, "status": self.account.status}
