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
