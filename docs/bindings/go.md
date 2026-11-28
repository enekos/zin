---
title: Go
parent: Bindings
nav_order: 4
---

# Go
{: .no_toc }

`github.com/enekos/zin/bindings/go` runs a specification's plan as subtests
of `go test`. Standard library only, Go 1.21+.
{: .fs-5 .fw-300 }

## Contents
{: .no_toc .text-delta }

1. TOC
{:toc}

---

## Install

```sh
go get github.com/enekos/zin/bindings/go
cargo install zin-spec        # the zin binary, unless you commit plans
```

## Minimal example

```go
package bank

import (
	"errors"
	"fmt"
	"testing"

	zin "github.com/enekos/zin/bindings/go"
)

type adapter struct{ account Account }

func (a *adapter) Call(action string, args zin.Args) error {
	switch action {
	case "deposit":
		if err := a.account.Deposit(args.Int("amount")); err != nil {
			return fmt.Errorf("%w: %v", zin.ErrRejected, err)
		}
		return nil
	}
	return fmt.Errorf("unknown action %s", action)
}

func (a *adapter) Observe() map[string]any {
	return map[string]any{"balance": a.account.Balance}
}

func TestAccount(t *testing.T) {
	zin.Verify(t, "account.zin", func() zin.Adapter { return &adapter{} })
}
```

The specification path is relative to the package directory, which is the
working directory of `go test`. `zin init go account.zin` prints a skeleton.

## The adapter

```go
type Adapter interface {
	Call(action string, args Args) error
}

type Observer interface { // optional
	Observe() map[string]any
}
```

The factory `func() zin.Adapter` runs once per plan test, and once more to
replay a shorter reproduction after a failure. There is no teardown hook: put
cleanup that must run in `t.Cleanup` from the enclosing test, or make the
factory's resources cheap.

### How arguments arrive

`zin.Args` is a `map[string]any` keyed by parameter name, with typed
accessors:

| Accessor | Specification type | Go value |
| --- | --- | --- |
| `args.Int("amount")` | integer | `int` (stored as `int64`) |
| `args.Bool("express")` | boolean | `bool` |
| `args.String("m")` | enum value | `string`, the name in the specification |

An accessor panics on a name the action does not have or a value of another
type, which fails the test with the message `zin: the action has no parameter "x"`
or `zin: parameter "x" is not an integer`.

### How the implementation says no

| `Call` returns | Means |
| --- | --- |
| `nil` | accepted |
| an error for which `errors.Is(err, zin.ErrRejected)` | rejected; the state must be unchanged |
| any other error | the test fails with `<call> failed: <error>` and the steps taken |

Wrap your domain error so both stay visible:

```go
if errors.Is(err, ErrInsufficientFunds) {
	return fmt.Errorf("%w: %v", zin.ErrRejected, err)
}
return err
```

### What Observe may return

A `map[string]any` keyed by state field names; only those fields are
compared. Values are normalized:

| Go value | Compared as |
| --- | --- |
| any integer kind | an integer |
| `bool`, `string`, named string types | themselves |
| a `fmt.Stringer` | its `String()` (enums) |
| `nil`, a nil pointer | nothing |
| a non-nil pointer | what it points to |
| `map[K]V` | a map, keys as text; lists every key |
| a struct | a record of its exported fields, named by a `zin` tag, else a `json` tag, else the field name with a lower-case first letter |
| a slice or array | a list, in order |
| `zin.Set{...}` | a set, in any order |

```go
func (a *adapter) Observe() map[string]any {
	waiting := zin.Set{}
	for _, m := range a.book.Waiting {
		waiting = append(waiting, m)
	}
	return map[string]any{"holder": a.book.Holder, "renewals": a.book.Renewals, "waiting": waiting}
}
```

Here `Holder` is a `*string`: nil is nothing, otherwise the string.

## Running and filtering

Each plan test is a subtest; Go replaces spaces with underscores in subtest
names:

```console
$ go test -v ./...
=== RUN   TestLoan
=== RUN   TestLoan/scenario:_A_reservation_blocks_renewal
=== RUN   TestLoan/scenario:_Only_the_holder's_book_can_come_back
=== RUN   TestLoan/reaches:_sometimes_renewals_is_2
=== RUN   TestLoan/tour_1
--- PASS: TestLoan (0.01s)
```

```console
$ go test -run 'TestLoan/tour_1' ./...
```

## Options

`Verify`, `Check` and `LoadPlan` take functional options:

| Option | `zin plan` flag |
| --- | --- |
| `zin.WithCover("states")` | `--cover` |
| `zin.WithMaxSteps(50)` | `--max-steps` |
| `zin.WithStepBudget(n)` | `--step-budget` |
| `zin.WithPairLimit(n)` | `--pair-limit` |
| `zin.WithComponent("Client")` | `--component` |
| `zin.WithPlan("loan.plan.json")` | an explicit plan file |
| `zin.WithBinary("/usr/local/bin/zin")` | the binary (default `$ZIN_BIN`, then `zin` on `PATH`) |

Numeric options must be positive (`zin: max steps must be a positive integer, not 0`).
Committed plans follow [the usual rules](index.md#where-the-plan-comes-from).

## API

| Function | Meaning |
| --- | --- |
| `Verify(t, spec, factory, options...)` | one subtest per plan test; `t.Fatal` on any failure |
| `Check(spec, factory, options...) error` | runs everything; returns the first `*Divergence`, or the adapter's own error |
| `LoadPlan(spec, options...) (*Plan, error)` | the plan, following the runner contract |
| `Fingerprint(source []byte) string` | the FNV-1a 64 a plan records |

```go
var divergence *zin.Divergence
if errors.As(zin.Check("loan.zin", newAdapter), &divergence) {
	fmt.Println(divergence.Scenario)    // the failing steps as a pasteable Zin scenario
	fmt.Println(divergence.Fingerprint) // the specification version
}
```

`Divergence.Report` is the full text that `Error()` returns. `zin.Plan`
mirrors the [`plan/1`](../plan-format.md#plan1) document.

## A complete example

The [Library loan](python.md#a-complete-example) specification against Go:

```go
package library

import "errors"

var ErrRefused = errors.New("refused")

type Book struct {
	Holder   *string
	Renewals int
	Waiting  []string
}

func (b *Book) Borrow(m string) error {
	if b.Holder != nil || (len(b.Waiting) > 0 && !contains(b.Waiting, m)) {
		return ErrRefused
	}
	b.Holder = &m
	b.Waiting = remove(b.Waiting, m)
	return nil
}

func (b *Book) Renew() error {
	if b.Holder == nil || len(b.Waiting) > 0 || b.Renewals >= 2 {
		return ErrRefused
	}
	b.Renewals++
	return nil
}

func (b *Book) GiveBack() error {
	if b.Holder == nil {
		return ErrRefused
	}
	b.Holder, b.Renewals = nil, 0
	return nil
}

func (b *Book) Reserve(m string) error {
	if b.Holder == nil || *b.Holder == m || contains(b.Waiting, m) {
		return ErrRefused
	}
	b.Waiting = append(b.Waiting, m)
	return nil
}

// contains and remove are ordinary slice helpers.
```

```go
package library

import (
	"errors"
	"fmt"
	"testing"

	zin "github.com/enekos/zin/bindings/go"
)

type adapter struct{ book Book }

func (a *adapter) Call(action string, args zin.Args) error {
	var err error
	switch action {
	case "borrow":
		err = a.book.Borrow(args.String("m"))
	case "renew":
		err = a.book.Renew()
	case "give_back":
		err = a.book.GiveBack()
	case "reserve":
		err = a.book.Reserve(args.String("m"))
	default:
		return fmt.Errorf("unknown action %s", action)
	}
	if errors.Is(err, ErrRefused) {
		return fmt.Errorf("%w: %v", zin.ErrRejected, err)
	}
	return err
}

func (a *adapter) Observe() map[string]any {
	waiting := zin.Set{}
	for _, m := range a.book.Waiting {
		waiting = append(waiting, m)
	}
	return map[string]any{"holder": a.book.Holder, "renewals": a.book.Renewals, "waiting": waiting}
}

func TestLoan(t *testing.T) {
	zin.Verify(t, "loan.zin", func() zin.Adapter { return &adapter{} })
}
```

## Troubleshooting

| Message | Cause |
| --- | --- |
| `<call> failed: <error>` with the steps taken | `Call` returned an error that does not wrap `zin.ErrRejected` |
| `Observe() returned "x", which is not a state field of the specification` | a misspelled key |
| `zin: cannot read specification loan.zin` | the path is not relative to the package directory |
| `zin: ...the zin binary was not found. Install zin-spec, set ZIN_BIN, or regenerate the plan: ...` | no usable plan, no binary |
| `zin: ... was derived from a different version of ...; regenerate it with: ...` | a stale explicit plan |
| `zin: zin plan loan.zin failed:` followed by checker output | the specification does not check |
| `zin: planning options need a .zin specification, not a plan file` | options with a `.json` path |
