# zin (Go)

Run a [Zin](https://github.com/enekos/zin) specification as conformance
subtests of `go test`. Standard library only.

```go
func TestAccount(t *testing.T) {
	zin.Verify(t, "account.zin", func() zin.Adapter { return &adapter{} })
}
```

An `Adapter` implements `Call(action string, args zin.Args) error` and returns
an error wrapping `zin.ErrRejected` when the implementation refuses; implement
`Observe() map[string]any` to have state compared. The plan comes from the
`zin` binary (`ZIN_BIN` or `PATH`) or from a committed `account.plan.json`. See
[conformance testing](https://github.com/enekos/zin/blob/main/docs/conformance.md).

## Planning options and committed plans

`WithCover`, `WithMaxSteps`, `WithStepBudget`, `WithPairLimit` and
`WithComponent` pass the options of `zin plan`. A committed sibling
`account.plan.json` (or `WithPlan`) is used when it is fresh for the
specification and every included file, and was derived with each option you
pass; options you leave out are not compared. A path to a `.json` plan replays
that plan, after checking it against `account.zin` beside it. See
[the runner contract](../../docs/plan-format.md#what-a-binding-must-do).

Use `errors.As` to get the `*zin.Divergence`: its `Scenario` is the failing
prefix as a Zin scenario, ready to paste, and `Fingerprint` names the
specification version.
