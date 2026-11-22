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

Select one component with `zin.WithComponent("Client")`. Use `zin.WithCover("pairs")` for adjacent calls, `zin.WithPairLimit(n)` for
a deterministic sample, `zin.WithMaxSteps(n)` for a soft tour target and
`zin.WithStepBudget(n)` for a hard replay budget. Source-backed plan loading
checks included file fingerprints and planning options. Passing a JSON plan
path directly replays that artifact. Use `errors.As` to access the failing
prefix as a pasteable scenario and its specification fingerprint on
`*zin.Divergence`.
