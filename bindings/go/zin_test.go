package zin_test

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	zin "github.com/enekos/zin/bindings/go"
)

const accountSpec = "../../examples/account.zin"

// --- a small implementation to verify, with switchable bugs ---------------

type status int

const (
	open status = iota
	closed
)

func (s status) String() string { return [...]string{"open", "closed"}[s] }

var errRefused = errors.New("refused")

type account struct {
	balance int
	status  status
	bug     string
}

func (a *account) deposit(amount int) error {
	limit := 20
	if a.bug == "cap" {
		limit = 21
	}
	if a.status != open || a.balance+amount > limit {
		return errRefused
	}
	a.balance += amount
	return nil
}

func (a *account) withdraw(amount int) error {
	if a.status != open || amount > a.balance {
		return errRefused
	}
	a.balance -= amount
	return nil
}

func (a *account) close() error {
	if a.status != open || (a.balance != 0 && a.bug != "close") {
		return errRefused
	}
	a.status = closed
	return nil
}

func (a *account) reopen() error {
	if a.status != closed {
		return errRefused
	}
	a.status = open
	if a.bug == "bonus" {
		a.balance++
	}
	return nil
}

// --- what a user writes ----------------------------------------------------

type adapter struct{ account account }

func (a *adapter) Call(action string, args zin.Args) error {
	var err error
	switch action {
	case "deposit":
		err = a.account.deposit(args.Int("amount"))
	case "withdraw":
		err = a.account.withdraw(args.Int("amount"))
	case "close":
		err = a.account.close()
	case "reopen":
		err = a.account.reopen()
	default:
		return fmt.Errorf("unknown action %s", action)
	}
	if errors.Is(err, errRefused) {
		return fmt.Errorf("%w: %v", zin.ErrRejected, err)
	}
	return err
}

func (a *adapter) Observe() map[string]any {
	return map[string]any{"balance": a.account.balance, "status": a.account.status}
}

func TestAccountConformsToItsSpecification(t *testing.T) {
	zin.Verify(t, accountSpec, func() zin.Adapter { return &adapter{} })
}

// --- the binding's own tests ----------------------------------------------

// blind hides Observe, so only accept/reject behavior is compared.
type blind struct{ inner *adapter }

func (b blind) Call(action string, args zin.Args) error { return b.inner.Call(action, args) }

func factory(bug string, observable bool) func() zin.Adapter {
	return func() zin.Adapter {
		inner := &adapter{account: account{bug: bug}}
		if observable {
			return inner
		}
		return blind{inner}
	}
}

func divergence(t *testing.T, err error, wanted ...string) {
	t.Helper()
	var diverged *zin.Divergence
	if !errors.As(err, &diverged) {
		t.Fatalf("expected a divergence, got %v", err)
	}
	for _, text := range wanted {
		if !strings.Contains(diverged.Report, text) {
			t.Fatalf("report lacks %q:\n%s", text, diverged.Report)
		}
	}
}

func TestCorrectImplementationConforms(t *testing.T) {
	for _, observable := range []bool{true, false} {
		if err := zin.Check(accountSpec, factory("", observable)); err != nil {
			t.Fatal(err)
		}
	}
	if err := zin.Check(accountSpec, factory("", true), zin.WithCover("states")); err != nil {
		t.Fatal(err)
	}
}

func TestOffByOneIsCaught(t *testing.T) {
	for _, observable := range []bool{true, false} {
		divergence(t, zin.Check(accountSpec, factory("cap", observable)),
			"accepted deposit(", "but the specification rejects it", "steps taken:")
	}
}

func TestMissingGuardIsCaught(t *testing.T) {
	divergence(t, zin.Check(accountSpec, factory("close", true)), "the implementation accepted close")
}

func TestWrongStateIsCaughtByObserve(t *testing.T) {
	divergence(t, zin.Check(accountSpec, factory("bonus", true)),
		"after reopen, balance is 1, but the specification says 0")
}

type crashing struct{}

func (crashing) Call(string, zin.Args) error { return errors.New("boom") }

func TestOtherErrorsAreNotRejections(t *testing.T) {
	err := zin.Check(accountSpec, func() zin.Adapter { return crashing{} })
	var diverged *zin.Divergence
	if err == nil || errors.As(err, &diverged) || !strings.Contains(err.Error(), "boom") {
		t.Fatalf("expected the adapter's error, got %v", err)
	}
}

func TestFingerprintMatchesTheChecker(t *testing.T) {
	plan, err := zin.LoadPlan(accountSpec)
	if err != nil {
		t.Fatal(err)
	}
	source, _ := os.ReadFile(accountSpec)
	if plan.Fingerprint != zin.Fingerprint(source) {
		t.Fatalf("%s != %s", plan.Fingerprint, zin.Fingerprint(source))
	}
	if zin.Fingerprint([]byte("a")) != "af63dc4c8601ec8c" {
		t.Fatal(zin.Fingerprint([]byte("a")))
	}
	if plan.Summary.Transitions != 182 {
		t.Fatal(plan.Summary)
	}
}

func TestCommittedPlanIsUsedWithoutTheBinary(t *testing.T) {
	directory := t.TempDir()
	spec := filepath.Join(directory, "account.zin")
	source, _ := os.ReadFile(accountSpec)
	if err := os.WriteFile(spec, source, 0o600); err != nil {
		t.Fatal(err)
	}
	binary := os.Getenv("ZIN_BIN")
	if binary == "" {
		binary = "zin"
	}
	planPath := filepath.Join(directory, "account.plan.json")
	if _, err := zin.LoadPlan(spec); err != nil {
		t.Fatal(err)
	}
	derived, err := os.ReadFile(planPath)
	if err == nil {
		t.Fatalf("LoadPlan must not write files, found %d bytes", len(derived))
	}
	// Derive and commit a plan the way a user would.
	out, err := runZin(binary, "plan", spec, "--out", planPath)
	if err != nil {
		t.Fatalf("%v: %s", err, out)
	}
	missing := zin.WithBinary(filepath.Join(directory, "no-such-zin"))
	if _, err := zin.LoadPlan(spec, missing); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(spec, append(source, []byte("\n# edited\n")...), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := zin.LoadPlan(spec, missing); err == nil || !strings.Contains(err.Error(), "out of date") {
		t.Fatalf("expected a stale-plan error, got %v", err)
	}
	if _, err := zin.LoadPlan(spec, zin.WithPlan(planPath)); err == nil || !strings.Contains(err.Error(), "different version") {
		t.Fatalf("expected a version error, got %v", err)
	}
}

func TestInvalidSpecificationFailsWithTheCheckersMessage(t *testing.T) {
	_, err := zin.LoadPlan("../../examples/broken/lost-update.zin")
	if err == nil || !strings.Contains(err.Error(), "counterexample") {
		t.Fatalf("expected the checker's counterexample, got %v", err)
	}
}

// --- maps and enum arguments ----------------------------------------------

type counter struct {
	value  int
	seen   map[string]int
	phase  map[string]string
	locked bool
	racy   bool
}

func (c *counter) Call(action string, args zin.Args) error {
	worker := args.String("w")
	switch action {
	case "read":
		if c.phase[worker] != "idle" || (c.locked && !c.racy) {
			return zin.ErrRejected
		}
		c.locked = true
		c.seen[worker] = c.value
		c.phase[worker] = "holding"
	case "write":
		if c.phase[worker] != "holding" {
			return zin.ErrRejected
		}
		c.value = c.seen[worker] + 1
		c.phase[worker] = "done"
		c.locked = false
	}
	return nil
}

func (c *counter) Observe() map[string]any {
	return map[string]any{"counter": c.value, "seen": c.seen, "at": c.phase, "locked": c.locked}
}

func newCounter(racy bool) func() zin.Adapter {
	return func() zin.Adapter {
		return &counter{
			seen:  map[string]int{"alice": 0, "bob": 0},
			phase: map[string]string{"alice": "idle", "bob": "idle"},
			racy:  racy,
		}
	}
}

func TestLockedCounterConforms(t *testing.T) {
	zin.Verify(t, "../../examples/counter.zin", newCounter(false))
}

func TestRacyCounterIsCaught(t *testing.T) {
	divergence(t, zin.Check("../../examples/counter.zin", newCounter(true)),
		"the implementation accepted read(", "but the specification rejects it")
}
