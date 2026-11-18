"""A small implementation to verify, with switchable bugs for the binding's own tests."""

import enum


class Status(enum.Enum):
    OPEN = "open"
    CLOSED = "closed"


class Account:
    LIMIT = 20

    def __init__(self, bug=None):
        self.balance = 0
        self.status = Status.OPEN
        self.bug = bug

    def deposit(self, amount):
        limit = self.LIMIT + 1 if self.bug == "cap" else self.LIMIT
        if self.status is not Status.OPEN or self.balance + amount > limit:
            raise ValueError("deposit refused")
        self.balance += amount

    def withdraw(self, amount):
        if self.status is not Status.OPEN or amount > self.balance:
            raise ValueError("withdrawal refused")
        self.balance -= amount
        if self.bug == "fee" and self.balance >= 10:
            self.balance -= 1

    def close(self):
        if self.status is not Status.OPEN:
            raise ValueError("already closed")
        if self.balance != 0 and self.bug != "close":
            raise ValueError("account is not empty")
        self.status = Status.CLOSED

    def reopen(self):
        if self.status is not Status.CLOSED:
            raise ValueError("not closed")
        self.status = Status.OPEN
        if self.bug == "bonus":
            self.balance += 1
