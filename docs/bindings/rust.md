---
title: Rust
parent: Bindings
nav_order: 5
---

# Rust
{: .no_toc }

The Rust binding is the checker itself: `zin::conform` parses and proves the
specification, derives the plan and replays it in-process, from an ordinary
`#[test]`. Nothing to install on test machines, nothing to commit.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

The crate is `zin-spec`; its library is named `zin`.

```toml
[dev-dependencies]
zin = { package = "zin-spec", version = "*" }   # pin a version in practice
```

Rust 1.88+.

## Minimal example

```rust
use zin::conform::{Adapter, Args, Outcome, assert_conforms};

#[derive(Default)]
struct Harness {
    account: Account,
}

impl Adapter for Harness {
    fn call(&mut self, action: &str, args: &Args) -> Outcome {
        match action {
            "close" => Outcome::refused_if(self.account.close(), |error| {
                matches!(error, AccountError::NotEmpty | AccountError::Closed)
            }),
            other => panic!("unknown action {other}"),
        }
    }
}

#[test]
fn conforms() {
    assert_conforms(include_str!("../account.zin"), Harness::default);
}
```

`zin init rust account.zin` prints a skeleton that uses
`assert_conforms_file` with the specification in `tests/`.

## The adapter

```rust
pub trait Adapter {
    fn call(&mut self, action: &str, args: &Args) -> Outcome;

    fn observe(&self) -> Option<State> {
        None
    }
}
```

The factory (`impl FnMut() -> A`) builds a fresh adapter per plan test, and
one more to replay a shorter reproduction after a failure. Cleanup belongs in
`Drop`.

### How arguments arrive

`Args` is `BTreeMap<String, Value>` keyed by parameter name. Arguments are
always scalars:

| Specification | `zin::Value` |
| --- | --- |
| integer | `Value::Integer(i64)` |
| boolean | `Value::Boolean(bool)` |
| enum value | `Value::Symbol(String)` |

```rust
fn member(args: &Args) -> &str {
    match &args["m"] {
        Value::Symbol(name) => name,
        other => panic!("m is not a member: {other:?}"),
    }
}
```

### How the implementation says no

`call` returns an `Outcome`, `Accepted` or `Rejected`. A rejected call must
leave the implementation unchanged. Three ways to build one:

| From | Gives |
| --- | --- |
| `bool` (`.into()`) | `true` accepted, `false` rejected |
| `Result<T, Refused>` (`.into()`) | `Ok` accepted, `Err(Refused)` rejected |
| `Outcome::refused_if(result, \|e\| ...)` | `Ok` accepted; an error the closure recognizes rejected; **any other error panics** |

Only `Result<_, Refused>` converts directly. Any other error type must say
which of its errors are refusals, so an I/O timeout can never pass as a
rejection: `Result<(), std::io::Error>` does not convert into an `Outcome`, by
design. A panic in `call` is a crash and fails the test.

### Observing state

`observe` returns `Some(State)` with any subset of the fields, or `None`.
`State` is `BTreeMap<String, Value>`:

| Specification | `Value` |
| --- | --- |
| integer, boolean, enum | `Integer`, `Boolean`, `Symbol` |
| optional | the value, or `Value::Nothing` |
| map, record | `Value::Map(BTreeMap<String, Value>)`; may be partial |
| set | `Value::List`, in any order |
| list | `Value::List`, in order |
| case with members | `Value::Tagged { tag, members }` |

```rust
fn observe(&self) -> Option<zin::State> {
    let holder = match &self.book.holder {
        Some(name) => Value::Symbol(name.clone()),
        None => Value::Nothing,
    };
    let waiting = self.book.waiting.iter().cloned().map(Value::Symbol).collect();
    Some(zin::State::from([
        ("holder".to_owned(), holder),
        ("renewals".to_owned(), Value::Integer(self.book.renewals.into())),
        ("waiting".to_owned(), Value::List(waiting)),
    ]))
}
```

## Functions

| Function | Meaning |
| --- | --- |
| `assert_conforms(source, factory) -> Conformance` | parse `source` (self-contained text), prove it, replay the default plan; panic with the report on any failure |
| `assert_conforms_file(path, factory) -> Conformance` | the same, reading `path` and every file it `include`s |
| `check(source, &PlanOptions, factory) -> Result<Conformance>` | with options; returns the error instead of panicking |
| `check_file(path, &PlanOptions, factory) -> Result<Conformance>` | the same, from a file |

`Conformance` reports `tests`, `accepted_steps` and `rejected_steps`. The
error of a failed `check` is a `zin::Error` whose text is the
[divergence report](index.md#anatomy-of-a-divergence).

A specification that uses `include` needs its files, not just its text: use
the `_file` forms with a path that does not depend on the working directory:

```rust
assert_conforms_file(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/loan.zin"), Harness::default);
```

`include_str!` plus `assert_conforms` fails on an `include` directive.

### Plan options

```rust
use zin::{Cover, PlanOptions};
use zin::conform::check_file;

let quick = PlanOptions::cover(Cover::States);
let pairs = PlanOptions::cover(Cover::Pairs { limit: Some(50) });
check_file("tests/loan.zin", &quick, Harness::default)?;
```

`PlanOptions` has `cover`, `max_steps` (default 200) and `step_budget`
(default and maximum 2,000,000). The struct is `#[non_exhaustive]`: start from
`PlanOptions::default()` or `PlanOptions::cover(...)` and set fields on it
(`options.max_steps = 50`). `Cover` is `Transitions`, `States`, `Actions`,
`Scenarios` or `Pairs { limit: Option<usize> }`.

### One component of a composition

`zin::conform` replays whole systems. To test one block of a composition,
verify it with `zin::verify_component(&spec, "Client")` and build the plan with
`Plan::build(&verification, &options)`; see
[testing one component](../language.md#testing-one-component).

## A complete example

The [Library loan](python.md#a-complete-example) specification, in
`tests/loan.zin`, against a Rust type:

```rust
// src/lib.rs
#[derive(Debug, PartialEq, Eq)]
pub enum LoanError {
    Refused(&'static str),
}

#[derive(Default)]
pub struct Book {
    pub holder: Option<String>,
    pub renewals: u8,
    pub waiting: Vec<String>,
}

impl Book {
    pub fn borrow(&mut self, member: &str) -> Result<(), LoanError> {
        if self.holder.is_some() || (!self.waiting.is_empty() && !self.waiting.iter().any(|m| m == member)) {
            return Err(LoanError::Refused("not available"));
        }
        self.holder = Some(member.to_owned());
        self.waiting.retain(|m| m != member);
        Ok(())
    }

    pub fn renew(&mut self) -> Result<(), LoanError> {
        if self.holder.is_none() || !self.waiting.is_empty() || self.renewals >= 2 {
            return Err(LoanError::Refused("cannot renew"));
        }
        self.renewals += 1;
        Ok(())
    }

    pub fn give_back(&mut self) -> Result<(), LoanError> {
        if self.holder.take().is_none() {
            return Err(LoanError::Refused("not on loan"));
        }
        self.renewals = 0;
        Ok(())
    }

    pub fn reserve(&mut self, member: &str) -> Result<(), LoanError> {
        if self.holder.as_deref().is_none_or(|h| h == member) || self.waiting.iter().any(|m| m == member) {
            return Err(LoanError::Refused("cannot reserve"));
        }
        self.waiting.push(member.to_owned());
        Ok(())
    }
}
```

```rust
// tests/conformance.rs
use library::{Book, LoanError};
use zin::Value;
use zin::conform::{Adapter, Args, Outcome, assert_conforms_file};

#[derive(Default)]
struct Harness {
    book: Book,
}

fn member(args: &Args) -> &str {
    match &args["m"] {
        Value::Symbol(name) => name,
        other => panic!("m is not a member: {other:?}"),
    }
}

impl Adapter for Harness {
    fn call(&mut self, action: &str, args: &Args) -> Outcome {
        let result = match action {
            "borrow" => self.book.borrow(member(args)),
            "renew" => self.book.renew(),
            "give_back" => self.book.give_back(),
            "reserve" => self.book.reserve(member(args)),
            other => panic!("unknown action {other}"),
        };
        Outcome::refused_if(result, |error| matches!(error, LoanError::Refused(_)))
    }

    fn observe(&self) -> Option<zin::State> {
        let holder = match &self.book.holder {
            Some(name) => Value::Symbol(name.clone()),
            None => Value::Nothing,
        };
        let waiting = self.book.waiting.iter().cloned().map(Value::Symbol).collect();
        Some(zin::State::from([
            ("holder".to_owned(), holder),
            ("renewals".to_owned(), Value::Integer(self.book.renewals.into())),
            ("waiting".to_owned(), Value::List(waiting)),
        ]))
    }
}

#[test]
fn conforms_to_specification() {
    assert_conforms_file(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/loan.zin"), Harness::default);
}
```

```console
$ cargo test --test conformance
test conforms_to_specification ... ok
```

## Troubleshooting

| Panic | Cause |
| --- | --- |
| `the implementation failed instead of refusing: <error>` | `refused_if` got an error its closure does not recognize |
| `observe() returned 'x', which is not a state field of the specification` | a misspelled key |
| a `violation:` or `spec:` error instead of a divergence | the specification itself does not check; run `zin check` |
| `unknown action <name>` | your own `panic!` arm: the specification has an action the harness does not handle |
