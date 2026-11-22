package zin

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestSharedObservationContract(t *testing.T) {
	data, err := os.ReadFile("../../tests/fixtures/observations.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name               string
		Expected, Observed map[string]any
		Matches            bool
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		if got := same(plain(c.Expected), plain(c.Observed)); got != c.Matches {
			t.Errorf("%s: got %v", c.Name, got)
		}
	}
}

func TestCachedOptionsAndSources(t *testing.T) {
	dir := t.TempDir()
	spec := filepath.Join(dir, "switch.zin")
	path := filepath.Join(dir, "switch.plan.json")
	for _, dst := range []string{spec, path} {
		data, err := os.ReadFile(filepath.Join("../../tests/fixtures/v0.2", filepath.Base(dst)))
		if err != nil {
			t.Fatal(err)
		}
		if err = os.WriteFile(dst, data, 0600); err != nil {
			t.Fatal(err)
		}
	}
	opts := []Option{WithCover("scenarios"), WithBinary("/no/zin")}
	p, err := LoadPlan(spec, opts...)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := LoadPlan(spec, append(opts, WithMaxSteps(1))...); err == nil {
		t.Fatal("stale tour target")
	}
	if _, err := LoadPlan(spec, WithPlan(path), WithStepBudget(1)); err == nil {
		t.Fatal("stale budget")
	}
	p.Sources = map[string]string{"child.zin": Fingerprint([]byte("child"))}
	data, err := json.Marshal(p)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	child := filepath.Join(dir, "child.zin")
	if err = os.WriteFile(child, []byte("child"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = LoadPlan(spec, opts...); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(child, []byte("edited"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = LoadPlan(spec, WithPlan(path)); err == nil {
		t.Fatal("stale child")
	}
}

func TestLargeUnsignedObservationDoesNotWrapToANegativeInteger(t *testing.T) {
	if same(int64(-1), plain(^uint64(0))) {
		t.Fatal("unsigned observation compared equal to -1")
	}
}

func TestComponentSelection(t *testing.T) {
	dir := t.TempDir()
	spec := filepath.Join(dir, "protocol.zin")
	data, err := os.ReadFile("../../examples/request-reply.zin")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(spec, data, 0600); err != nil {
		t.Fatal(err)
	}
	plan, err := LoadPlan(spec, WithComponent("Client"))
	if err != nil {
		t.Fatal(err)
	}
	if plan.Component != "Client" {
		t.Fatalf("wrong component: %s", plan.Component)
	}
	data, err = json.Marshal(plan)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "protocol.plan.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = LoadPlan(spec, WithComponent("Client"), WithBinary("/no/zin")); err != nil {
		t.Fatal(err)
	}
	if _, err = LoadPlan(spec, WithBinary("/no/zin")); err == nil {
		t.Fatal("whole-system run reused a component plan")
	}
}
