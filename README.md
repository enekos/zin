# Zin

**Behavior you can read, run, and prove.**

**Zin** is Basque for a solemn promise. Zin is an English-like formal specification language for finite-state systems. It gives product people, reviewers, and engineers one artifact that reads like a behavioral contract, executes like a model, and renders like a design document.

```zin
system "Bank account"

state
  balance is an integer from 0 to 20
  status is one of open, closed

initially
  balance is 0
  status is open

always
  balance is at least 0
  status is closed implies balance is 0

action deposit(amount is an integer from 1 to 5)
  requires status is open
  requires balance + amount is at most 20
  balance becomes balance + amount

action withdraw(amount is an integer from 1 to 5)
  requires status is open
  requires amount is at most balance
  balance becomes balance - amount

action close
  requires status is open
  requires balance is 0
  status becomes closed

scenario "A clean close"
  deposit with amount = 5
  withdraw with amount = 5
  close
  expect status is closed
```

Change the journey to close before withdrawing and Zin reports the exact disabled call and state instead of asking a reviewer to simulate it mentally.

## Why Zin

- **Readable without being vague.** Types, ranges, preconditions, invariants, and updates have exact semantics.
- **Statically sound specifications.** Every expression and enum domain is checked before exploration, even inside unreachable actions.
- **Exhaustive, not example-driven.** The checker explores every reachable state and every action argument within finite domains.
- **Failures tell a story.** Broken promises return a shortest counterexample trace and the offending state.
- **Diagrams are native.** One command produces portable Markdown with Mermaid state and sequence diagrams.
- **No target-stack assumptions.** Model a Rust service, a TypeScript UI, a protocol, a workflow, or a system that has no code yet.
- **No runtime dependencies.** The reference CLI is a small Rust crate using only the standard library.

## Install and run

Rust 1.85 or newer builds the reference tool:

```sh
cargo install --path .
zin check examples/account.zin
zin report examples/account.zin --out account-review.md
zin diagram examples/account.zin
zin diagram examples/account.zin --scenario "Deposit, withdraw, and close"
```

`report` verifies before writing. Its Markdown can live in any repository and render anywhere Mermaid is supported; the generated diagrams are also available as plain text for other tooling.

## What a check proves

For the declared model, Zin proves that every explored initial and reachable state satisfies every `always` promise. It also executes every scenario and expectation. Exploration is bounded only by the explicit state limit; reaching that limit is an error, never a silent partial pass.

Zin verifies the **specification**, not an implementation of it. Use scenarios as conformance-test blueprints and keep the `.zin` file beside the implementation for review. It currently handles finite safety properties, deterministic actions, integers, booleans, and enums—not liveness, probabilities, floating point, or unbounded data.

See [the language guide](docs/language.md) for the complete grammar and semantics.

See [integrating with a real app](docs/integration.md) for a small, dependency-free Python example covering the specification, implementation, conformance tests, and CI workflow.

The proposed [repository plugin architecture](docs/plugin-architecture.md) describes versioned application adapters, `zin conform`, machine-readable interfaces, and the delivery plan for integrating Zin with any codebase.

## Develop

```sh
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The language and generated formats are the product. Alternative implementations can target the documented grammar without depending on this Rust codebase.
