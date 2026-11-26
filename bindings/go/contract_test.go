package zin

// The runner contract shared by every binding: bindings/fixtures/contract.

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

var contract = filepath.Join("..", "fixtures", "contract")

type contractCases struct {
	Adapter script       `json:"adapter"`
	Load    []loadCase   `json:"load"`
	Replay  []replayCase `json:"replay"`
}

type loadCase struct {
	Name    string         `json:"name"`
	Spec    string         `json:"spec"`
	Plan    string         `json:"plan"`
	Options map[string]any `json:"options"`
	Expect  struct {
		Errors      []string `json:"errors"`
		Fingerprint string   `json:"fingerprint"`
		Cover       string   `json:"cover"`
	} `json:"expect"`
}

type rule struct {
	Requires map[string]any `json:"requires"`
	Set      map[string]any `json:"set"`
}

type override struct {
	Instance int    `json:"instance"`
	Call     int    `json:"call"`
	Action   string `json:"action"`
	Outcome  string `json:"outcome"`
	Message  string `json:"message"`
}

type script struct {
	Initial   map[string]any  `json:"initial"`
	Observe   *bool           `json:"observe"`
	Rules     map[string]rule `json:"rules"`
	Overrides []override      `json:"overrides"`
}

type replayCase struct {
	Name    string `json:"name"`
	Plan    string `json:"plan"`
	Tests   []int  `json:"tests"`
	Adapter script `json:"adapter"`
	Expect  struct {
		Passes    bool   `json:"passes"`
		Report    string `json:"report"`
		Raises    string `json:"raises"`
		Factories *int   `json:"factories"`
	} `json:"expect"`
}

func readCases(t *testing.T) contractCases {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(contract, "cases.json"))
	if err != nil {
		t.Fatal(err)
	}
	var cases contractCases
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	return cases
}

func contractOptions(t *testing.T, item loadCase) []Option {
	options := []Option{WithBinary(filepath.Join(contract, "no-such-zin"))}
	if item.Plan != "" {
		options = append(options, WithPlan(filepath.Join(contract, item.Plan)))
	}
	for key, value := range item.Options {
		switch key {
		case "cover":
			options = append(options, WithCover(value.(string)))
		case "component":
			options = append(options, WithComponent(value.(string)))
		case "max_steps":
			options = append(options, WithMaxSteps(int(value.(float64))))
		case "step_budget":
			options = append(options, WithStepBudget(int(value.(float64))))
		case "pair_limit":
			options = append(options, WithPairLimit(int(value.(float64))))
		default:
			t.Fatalf("unknown option %s", key)
		}
	}
	return options
}

func TestContractLoad(t *testing.T) {
	for _, item := range readCases(t).Load {
		t.Run(item.Name, func(t *testing.T) {
			plan, err := LoadPlan(filepath.Join(contract, item.Spec), contractOptions(t, item)...)
			if len(item.Expect.Errors) > 0 {
				if err == nil {
					t.Fatal("expected an error")
				}
				for _, fragment := range item.Expect.Errors {
					if !strings.Contains(err.Error(), fragment) {
						t.Fatalf("%q does not contain %q", err, fragment)
					}
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if plan.Fingerprint != item.Expect.Fingerprint || plan.Cover != item.Expect.Cover {
				t.Fatalf("got %s %s", plan.Fingerprint, plan.Cover)
			}
		})
	}
}

// scripted is the contract's table-driven adapter (see the contract README).
type scripted struct {
	script   script
	instance int
	calls    int
	state    map[string]any
}

func (s *scripted) Call(action string, _ Args) error {
	s.calls++
	for _, o := range s.script.Overrides {
		if (o.Instance == 0 || o.Instance == s.instance) && (o.Call == 0 || o.Call == s.calls) && (o.Action == "" || o.Action == action) {
			switch o.Outcome {
			case "raise":
				return errors.New(o.Message)
			case "accept":
				return nil
			default:
				return ErrRejected
			}
		}
	}
	rule := s.script.Rules[action]
	for key, value := range rule.Requires {
		if s.state[key] != value {
			return ErrRejected
		}
	}
	for key, value := range rule.Set {
		s.state[key] = value
	}
	return nil
}

type observing struct{ *scripted }

func (o observing) Observe() map[string]any {
	copied := make(map[string]any, len(o.state))
	for key, value := range o.state {
		copied[key] = value
	}
	return copied
}

func TestContractReplay(t *testing.T) {
	cases := readCases(t)
	for _, item := range cases.Replay {
		t.Run(item.Name, func(t *testing.T) {
			plan, err := LoadPlan(filepath.Join(contract, item.Plan))
			if err != nil {
				t.Fatal(err)
			}
			current := cases.Adapter
			if item.Adapter.Initial != nil {
				current.Initial = item.Adapter.Initial
			}
			if item.Adapter.Observe != nil {
				current.Observe = item.Adapter.Observe
			}
			current.Overrides = item.Adapter.Overrides
			made := 0
			factory := func() Adapter {
				made++
				state := make(map[string]any, len(current.Initial))
				for key, value := range current.Initial {
					state[key] = value
				}
				adapter := &scripted{script: current, instance: made, state: state}
				if *current.Observe {
					return observing{adapter}
				}
				return adapter
			}
			r, err := newRunner(plan)
			if err != nil {
				t.Fatal(err)
			}
			for _, index := range item.Tests {
				err := r.check(plan.Tests[index], factory)
				var divergence *Divergence
				switch {
				case item.Expect.Passes:
					if err != nil {
						t.Fatal(err)
					}
				case item.Expect.Raises != "":
					if err == nil || errors.As(err, &divergence) || errors.Unwrap(err).Error() != item.Expect.Raises {
						t.Fatalf("expected the adapter's error %q, got %v", item.Expect.Raises, err)
					}
				default:
					if !errors.As(err, &divergence) {
						t.Fatalf("expected a divergence, got %v", err)
					}
					if divergence.Report != item.Expect.Report {
						t.Fatalf("report:\n%s\nwant:\n%s", divergence.Report, item.Expect.Report)
					}
				}
			}
			if item.Expect.Factories != nil && made != *item.Expect.Factories {
				t.Fatalf("made %d adapters, want %d", made, *item.Expect.Factories)
			}
		})
	}
}

type token string

func (t token) String() string { return string(t) }

// native turns the fixture's $enum and $map markers into Go values: a named
// string type and maps with non-string keys.
func native(value any) any {
	object, ok := value.(map[string]any)
	if !ok {
		return value
	}
	if name, ok := object["$enum"]; ok {
		return token(name.(string))
	}
	if items, ok := object["$set"]; ok {
		out := Set{}
		for _, item := range items.([]any) {
			out = append(out, native(item))
		}
		return out
	}
	if members, ok := object["$record"]; ok {
		return native(members)
	}
	if entries, ok := object["$map"]; ok {
		out := map[any]any{}
		for _, entry := range entries.([]any) {
			pair := entry.([]any)
			out[native(pair[0])] = native(pair[1])
		}
		return out
	}
	out := make(map[string]any, len(object))
	for key, item := range object {
		out[key] = native(item)
	}
	return out
}

type fixedObserver map[string]any

func (fixedObserver) Call(string, Args) error   { return nil }
func (f fixedObserver) Observe() map[string]any { return f }

type observationCase struct {
	Name     string         `json:"name"`
	Expected map[string]any `json:"expected"`
	Observed map[string]any `json:"observed"`
	Matches  bool           `json:"matches"`
}

func TestContractObservations(t *testing.T) {
	var cases []observationCase
	for _, name := range []string{"observations.json", "native-observations.json"} {
		data, err := os.ReadFile(filepath.Join(contract, name))
		if err != nil {
			t.Fatal(err)
		}
		var some []observationCase
		if err := json.Unmarshal(data, &some); err != nil {
			t.Fatal(err)
		}
		cases = append(cases, some...)
	}
	for _, item := range cases {
		t.Run(item.Name, func(t *testing.T) {
			plan := &Plan{
				System:  "Observation",
				States:  []map[string]any{item.Expected},
				Parents: []*[2]int{nil},
				Tests:   []Test{{Name: "t"}},
			}
			r, err := newRunner(plan)
			if err != nil {
				t.Fatal(err)
			}
			observed := native(item.Observed).(map[string]any)
			err = r.check(plan.Tests[0], func() Adapter { return fixedObserver(observed) })
			if (err == nil) != item.Matches {
				t.Fatalf("matches = %v, want %v (%v)", err == nil, item.Matches, err)
			}
		})
	}
}

func TestContractPlansHaveTheSchemaKeys(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "..", "docs", "plan.schema.json"))
	if err != nil {
		t.Fatal(err)
	}
	var schema struct {
		Required   []string `json:"required"`
		Properties struct {
			Summary struct {
				Required []string `json:"required"`
			} `json:"summary"`
		} `json:"properties"`
	}
	if err := json.Unmarshal(data, &schema); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"latch.plan.json", "composed/main.plan.json"} {
		data, err := os.ReadFile(filepath.Join(contract, name))
		if err != nil {
			t.Fatal(err)
		}
		var top map[string]json.RawMessage
		if err := json.Unmarshal(data, &top); err != nil {
			t.Fatal(err)
		}
		for _, key := range schema.Required {
			if _, ok := top[key]; !ok {
				t.Fatalf("%s: missing %s", name, key)
			}
		}
		var summary map[string]any
		if err := json.Unmarshal(top["summary"], &summary); err != nil {
			t.Fatal(err)
		}
		for _, key := range schema.Properties.Summary.Required {
			if _, ok := summary[key]; !ok {
				t.Fatalf("%s: missing summary.%s", name, key)
			}
		}
	}
}

func TestStructsAreObservedAsRecords(t *testing.T) {
	type order struct {
		Status token `zin:"status"`
		Items  int
		secret int
	}
	got := plain(map[string]any{"order": order{Status: "paid", Items: 2, secret: 1}, "queue": []order{{Status: "open"}}})
	want := map[string]any{
		"order": map[string]any{"status": "paid", "items": int64(2)},
		"queue": []any{map[string]any{"status": "open", "items": int64(0)}},
	}
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("plain = %#v", got)
	}
	if !same(plain([]any{"ann", "ben"}), Set{"ben", "ann"}) || same(plain([]any{"ann", "ben"}), []any{"ben", "ann"}) {
		t.Fatal("a Set matches in any order, a slice in order")
	}
}
