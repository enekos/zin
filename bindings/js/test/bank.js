// A small implementation to verify, with switchable bugs for the binding's own tests.

export class Account {
  balance = 0;
  status = "open";

  constructor(bug = null) {
    this.bug = bug;
  }

  deposit(amount) {
    const limit = this.bug === "cap" ? 21 : 20;
    if (this.status !== "open" || this.balance + amount > limit) {
      throw new RangeError("deposit refused");
    }
    this.balance += amount;
  }

  async withdraw(amount) {
    await null; // implementations are often asynchronous
    if (this.status !== "open" || amount > this.balance) {
      throw new RangeError("withdrawal refused");
    }
    this.balance -= amount;
  }

  close() {
    if (this.status !== "open") throw new RangeError("already closed");
    if (this.balance !== 0 && this.bug !== "close") throw new RangeError("account is not empty");
    this.status = "closed";
  }

  reopen() {
    if (this.status !== "closed") throw new RangeError("not closed");
    this.status = "open";
    if (this.bug === "bonus") this.balance += 1;
  }
}

export function adapter(bug = null, observable = true) {
  return () => {
    const account = new Account(bug);
    return {
      actions: {
        deposit: ({ amount }) => account.deposit(amount),
        withdraw: ({ amount }) => account.withdraw(amount),
        close: () => account.close(),
        reopen: () => account.reopen(),
      },
      rejects: (error) => error instanceof RangeError,
      observe: observable ? () => ({ balance: account.balance, status: account.status }) : undefined,
    };
  };
}
