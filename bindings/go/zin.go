// Package zin runs a Zin specification's derived test plan against a Go
// implementation, as ordinary subtests of `go test`.
//
//	func TestAccount(t *testing.T) {
//		zin.Verify(t, "account.zin", func() zin.Adapter { return &adapter{} })
//	}
//
// The adapter maps each action of the specification onto the implementation.
// Return [ErrRejected] (or an error wrapping it) when the implementation
// refuses an action. Implement [Observer] to have state compared after every
// step.
package zin

import (
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"testing"
)

// PlanFormat is the plan file format this package reads.
const PlanFormat = "plan/1"

// listedSteps is how many of the latest steps a report lists before eliding
// earlier ones.
const listedSteps = 8

// ErrRejected reports that the implementation refused an action.
var ErrRejected = errors.New("zin: action rejected")

// Args holds one call's arguments by parameter name.
type Args map[string]any

func (a Args) value(name string) any {
	value, ok := a[name]
	if !ok {
		panic(fmt.Sprintf("zin: the action has no parameter %q", name))
	}
	return value
}

// Int returns an integer argument.
func (a Args) Int(name string) int {
	value, ok := a.value(name).(int64)
	if !ok {
		panic(fmt.Sprintf("zin: parameter %q is not an integer", name))
	}
	return int(value)
}

// Bool returns a boolean argument.
func (a Args) Bool(name string) bool {
	value, ok := a.value(name).(bool)
	if !ok {
		panic(fmt.Sprintf("zin: parameter %q is not a boolean", name))
	}
	return value
}

// String returns an enum argument as its name in the specification.
func (a Args) String(name string) string {
	value, ok := a.value(name).(string)
	if !ok {
		panic(fmt.Sprintf("zin: parameter %q is not an enum value", name))
	}
	return value
}

// Adapter connects the implementation under test to the specification.
type Adapter interface {
	// Call performs one action. A nil error means the implementation accepted
	// it; an error wrapping ErrRejected means it refused and changed nothing;
	// any other error fails the test.
	Call(action string, args Args) error
}

// Observer is optionally implemented by an Adapter to expose any subset of
// the specification's state fields. Integers, booleans, strings,
// fmt.Stringers and maps of those are understood.
type Observer interface {
	Observe() map[string]any
}

// Plan is a derived test plan, as written by `zin plan`.
type Plan struct {
	Format      string            `json:"zin"`
	System      string            `json:"system"`
	Fingerprint string            `json:"fingerprint"`
	Cover       string            `json:"cover"`
	MaxSteps    int               `json:"max_steps"`
	StepBudget  int               `json:"step_budget"`
	PairLimit   *int              `json:"pair_limit"`
	Component   string            `json:"component"`
	Sources     map[string]string `json:"sources"`
	Actions     []Action          `json:"actions"`
	States      []map[string]any  `json:"states"`
	Parents     []*[2]int         `json:"parents"`
	Calls       []Call            `json:"calls"`
	Tests       []Test            `json:"tests"`
	Summary     Summary           `json:"summary"`
}

// Action is one action of the specification.
type Action struct {
	Name   string `json:"name"`
	Params []struct {
		Name string `json:"name"`
	} `json:"params"`
}

// Call is an action with concrete arguments.
type Call struct {
	Action string `json:"action"`
	Args   []any  `json:"args"`
	Text   string `json:"text"`
}

// Test is one sequence of steps. A step is {call, to}; to is -1 when the call
// must be rejected.
type Test struct {
	Name  string   `json:"name"`
	Kind  string   `json:"kind"`
	Steps [][2]int `json:"steps"`
}

// Summary counts what a plan covers.
type Summary struct {
	States        int `json:"states"`
	Transitions   int `json:"transitions"`
	Tests         int `json:"tests"`
	AcceptedSteps int `json:"accepted_steps"`
	RejectedSteps int `json:"rejected_steps"`
}

type config struct {
	component                       *string
	cover                           string
	plan                            string
	binary                          string
	maxSteps, stepBudget, pairLimit *int
}

// Option adjusts how the plan is obtained.
type Option func(*config)

// WithCover selects the coverage goal: transitions (default), states,
// actions or scenarios.
func WithCover(cover string) Option { return func(c *config) { c.cover = cover } }

// WithPlan uses a pre-derived plan file instead of running `zin plan`.
func WithPlan(path string) Option { return func(c *config) { c.plan = path } }

// WithBinary sets the path of the zin binary.
func WithBinary(path string) Option { return func(c *config) { c.binary = path } }

// WithComponent derives the plan for one component of a composition.
func WithComponent(name string) Option { return func(c *config) { c.component = &name } }

// WithMaxSteps sets a soft target for each tour.
func WithMaxSteps(n int) Option { return func(c *config) { c.maxSteps = &n } }

// WithStepBudget limits the total number of steps across all tests.
func WithStepBudget(n int) Option { return func(c *config) { c.stepBudget = &n } }

// WithPairLimit selects a deterministic sample of adjacent calls.
func WithPairLimit(n int) Option { return func(c *config) { c.pairLimit = &n } }

// Fingerprint is the FNV-1a 64 hash of the specification bytes, as `zin plan`
// records it.
func Fingerprint(source []byte) string {
	value := uint64(0xcbf29ce484222325)
	for _, b := range source {
		value = (value ^ uint64(b)) * 0x100000001b3
	}
	return fmt.Sprintf("%016x", value)
}

func readPlan(path string) (*Plan, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("zin: cannot read plan: %w", err)
	}
	return parsePlan(data, path)
}

func parsePlan(data []byte, origin string) (*Plan, error) {
	var plan Plan
	if err := json.Unmarshal(data, &plan); err != nil {
		return nil, fmt.Errorf("zin: %s is not a plan: %w", origin, err)
	}
	if plan.Format != PlanFormat {
		return nil, fmt.Errorf("zin: %s is %q, but this package reads %s", origin, plan.Format, PlanFormat)
	}
	return &plan, nil
}

// LoadPlan returns the plan for spec: the WithPlan file when given, else a
// sibling <name>.plan.json that matches the specification, else the output
// of `zin plan` (WithBinary, $ZIN_BIN, or zin on PATH). spec may itself be a
// plan file.
func LoadPlan(spec string, options ...Option) (*Plan, error) {
	settings := config{}
	for _, option := range options {
		option(&settings)
	}
	if strings.HasSuffix(spec, ".json") {
		return readPlan(spec)
	}
	source, err := os.ReadFile(spec)
	if err != nil {
		return nil, fmt.Errorf("zin: cannot read specification: %w", err)
	}
	wanted := Fingerprint(source)
	cover := settings.cover
	if cover == "" {
		cover = "transitions"
	}
	args := []string{"plan", spec, "--cover", cover}
	for _, opt := range []struct {
		name  string
		value *int
	}{
		{"--max-steps", settings.maxSteps}, {"--step-budget", settings.stepBudget}, {"--pair-limit", settings.pairLimit},
	} {
		if opt.value != nil {
			if *opt.value < 1 {
				return nil, fmt.Errorf("zin: %s must be positive", opt.name)
			}
			args = append(args, opt.name, strconv.Itoa(*opt.value))
		}
	}
	component := ""
	if settings.component != nil {
		component = *settings.component
		args = append(args, "--component", component)
	}
	sibling := strings.TrimSuffix(spec, filepath.Ext(spec)) + ".plan.json"
	cached := settings.plan
	explicit := cached != ""
	if !explicit {
		cached = sibling
	}
	regenerate := "zin " + strings.Join(args, " ") + " --out " + cached
	stale := ""
	if _, err := os.Stat(cached); explicit || err == nil {
		plan, err := readPlan(cached)
		if err != nil {
			return nil, err
		}
		fresh := plan.Fingerprint == wanted
		for path, digest := range plan.Sources {
			data, err := os.ReadFile(filepath.Join(filepath.Dir(spec), filepath.FromSlash(path)))
			if err != nil || Fingerprint(data) != digest {
				fresh = false
				break
			}
		}
		maxSteps, budget := plan.MaxSteps, plan.StepBudget
		if maxSteps == 0 {
			maxSteps = 200
		}
		if budget == 0 {
			budget = 2_000_000
		}
		matches := func(wanted *int, fallback, actual int) bool {
			if wanted == nil {
				return explicit || fallback == actual
			}
			return *wanted == actual
		}
		pairLimit := 0
		if plan.PairLimit != nil {
			pairLimit = *plan.PairLimit
		}
		compatible := ((explicit && settings.cover == "") || plan.Cover == cover) &&
			((explicit && settings.component == nil) || plan.Component == component) &&
			matches(settings.maxSteps, 200, maxSteps) && matches(settings.stepBudget, 2_000_000, budget) && matches(settings.pairLimit, 0, pairLimit)
		if fresh && compatible {
			return plan, nil
		}
		if explicit {
			if !fresh {
				return nil, fmt.Errorf("zin: %s was derived from a different version of %s; regenerate with: %s", cached, spec, regenerate)
			}
			return nil, fmt.Errorf("zin: %s has different planning options; regenerate with: %s", cached, regenerate)
		}
		stale = filepath.Base(cached) + " is out of date and "
	}
	binary := settings.binary
	if binary == "" {
		binary = os.Getenv("ZIN_BIN")
	}
	if binary == "" {
		binary = "zin"
	}
	resolved, err := exec.LookPath(binary)
	if err != nil {
		return nil, fmt.Errorf(
			"zin: %sthe zin binary was not found. Install it (cargo install zin-spec), set ZIN_BIN, or commit a plan: %s",
			stale, regenerate)
	}
	command := exec.Command(resolved, args...)
	var stderr strings.Builder
	command.Stderr = &stderr
	output, err := command.Output()
	if err != nil {
		return nil, fmt.Errorf("zin: zin plan %s failed:\n%s", spec, strings.TrimSpace(stderr.String()))
	}
	return parsePlan(output, resolved)
}

// plain converts a value into the plan's vocabulary: int64, bool, string, or
// map[string]any of those.
func plain(value any) any {
	switch typed := value.(type) {
	case nil:
		return nil
	case float64:
		if typed == float64(int64(typed)) {
			return int64(typed)
		}
		return typed
	case fmt.Stringer:
		return typed.String()
	}
	reflected := reflect.ValueOf(value)
	switch reflected.Kind() {
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		return reflected.Int()
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		if n := reflected.Uint(); n <= math.MaxInt64 {
			return int64(n)
		}
		return reflected.Uint()
	case reflect.Bool:
		return reflected.Bool()
	case reflect.String:
		return reflected.String()
	case reflect.Map:
		out := make(map[string]any, reflected.Len())
		for _, key := range reflected.MapKeys() {
			out[fmt.Sprint(plain(key.Interface()))] = plain(reflected.MapIndex(key).Interface())
		}
		return out
	}
	return value
}

func show(value any) string {
	entries, ok := value.(map[string]any)
	if !ok {
		return fmt.Sprint(value)
	}
	keys := make([]string, 0, len(entries))
	for key := range entries {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		parts = append(parts, key+": "+show(entries[key]))
	}
	return "{" + strings.Join(parts, ", ") + "}"
}

func same(expected, observed any) bool {
	wanted, ok := expected.(map[string]any)
	if !ok {
		return expected == observed
	}
	actual, ok := observed.(map[string]any)
	if !ok {
		return false
	}
	for key, item := range actual {
		want, known := wanted[key]
		if !known || !same(want, item) {
			return false
		}
	}
	return true
}

type runner struct {
	plan   *Plan
	params map[string][]string
}

func newRunner(plan *Plan) (*runner, error) {
	params := make(map[string][]string, len(plan.Actions))
	for _, action := range plan.Actions {
		names := make([]string, 0, len(action.Params))
		for _, param := range action.Params {
			names = append(names, param.Name)
		}
		params[action.Name] = names
	}
	for _, test := range plan.Tests {
		for _, step := range test.Steps {
			if step[0] < 0 || step[0] >= len(plan.Calls) || step[1] >= len(plan.States) {
				return nil, fmt.Errorf("zin: the plan is malformed: step %v is out of range", step)
			}
		}
	}
	if len(plan.States) == 0 || len(plan.Parents) != len(plan.States) {
		return nil, errors.New("zin: the plan is malformed: states and parents do not line up")
	}
	return &runner{plan: plan, params: params}, nil
}

func (r *runner) text(step [2]int) string {
	text := r.plan.Calls[step[0]].Text
	if step[1] < 0 {
		return "reject " + text
	}
	return text
}

func (r *runner) listing(steps [][2]int) string {
	skipped := len(steps) - listedSteps
	var lines []string
	if skipped > 0 {
		lines = append(lines, fmt.Sprintf("  ... %d earlier steps", skipped))
	} else {
		skipped = 0
	}
	for index := skipped; index < len(steps); index++ {
		lines = append(lines, fmt.Sprintf("  %d. %s", index+1, r.text(steps[index])))
	}
	return strings.Join(lines, "\n")
}

func (r *runner) compare(adapter Adapter, state int, after string) string {
	observer, ok := adapter.(Observer)
	if !ok {
		return ""
	}
	observed := observer.Observe()
	fields := make([]string, 0, len(observed))
	for field := range observed {
		fields = append(fields, field)
	}
	sort.Strings(fields)
	expected := r.plan.States[state]
	for _, field := range fields {
		wanted, known := expected[field]
		if !known {
			return fmt.Sprintf("Observe() returned %q, which is not a state field of the specification", field)
		}
		actual := plain(observed[field])
		if !same(plain(wanted), actual) {
			return fmt.Sprintf("%s, %s is %s, but the specification says %s", after, field, show(actual), show(plain(wanted)))
		}
	}
	return ""
}

// run returns the number and description of the first diverging step, or an
// error when the adapter itself failed.
func (r *runner) run(adapter Adapter, steps [][2]int) (int, string, error) {
	if message := r.compare(adapter, 0, "before any action"); message != "" {
		return 0, message, nil
	}
	state := 0
	for index, step := range steps {
		call := r.plan.Calls[step[0]]
		names := r.params[call.Action]
		args := make(Args, len(call.Args))
		for position, value := range call.Args {
			if position < len(names) {
				args[names[position]] = plain(value)
			}
		}
		err := adapter.Call(call.Action, args)
		if err != nil && !errors.Is(err, ErrRejected) {
			return index + 1, "", fmt.Errorf("%s failed: %w\nsteps taken:\n%s", call.Text, err, r.listing(steps[:index+1]))
		}
		accepted := err == nil
		message := ""
		switch {
		case step[1] >= 0 && accepted:
			state = step[1]
			message = r.compare(adapter, state, "after "+call.Text)
		case step[1] < 0 && !accepted:
			message = r.compare(adapter, state, "after rejecting "+call.Text)
		case accepted:
			message = fmt.Sprintf("the implementation accepted %s, but the specification rejects it", call.Text)
		default:
			message = fmt.Sprintf("the implementation rejected %s, but the specification allows it", call.Text)
		}
		if message != "" {
			return index + 1, message, nil
		}
	}
	return 0, "", nil
}

func (r *runner) shortest(steps [][2]int, failed int) [][2]int {
	state := 0
	for _, step := range steps[:failed-1] {
		if step[1] >= 0 {
			state = step[1]
		}
	}
	var path [][2]int
	for r.plan.Parents[state] != nil {
		parent := r.plan.Parents[state]
		path = append(path, [2]int{parent[1], state})
		state = parent[0]
	}
	for left, right := 0, len(path)-1; left < right; left, right = left+1, right-1 {
		path[left], path[right] = path[right], path[left]
	}
	return append(path, steps[failed-1])
}

func (r *runner) check(test Test, factory func() Adapter) error {
	failed, message, err := r.run(factory(), test.Steps)
	if err != nil {
		return err
	}
	if message == "" {
		return nil
	}
	taken := test.Steps[:failed]
	report := fmt.Sprintf("%s: implementation diverges from the specification in test %q at step %d\n%s\nsteps taken:\n%s",
		r.plan.System, test.Name, failed, message, r.listing(taken))
	var scenario strings.Builder
	scenario.WriteString("scenario \"Replay divergence\"")
	for _, step := range taken {
		scenario.WriteString("\n  " + r.text(step))
	}
	if len(taken) == 0 {
		scenario.WriteString("\n  expect true")
	}
	report += "\nspec fingerprint: " + r.plan.Fingerprint + "\n" + scenario.String()
	if failed > 0 {
		if short := r.shortest(test.Steps, failed); len(short) < len(taken) {
			again, repeated, err := r.run(factory(), short)
			switch {
			case err != nil:
			case again == len(short) && repeated == message:
				report += "\nshortest reproduction:\n" + r.listing(short[:again])
			default:
				report += "\nthe shortest path to the same specification state does not reproduce the original mismatch"
			}
		}
	}
	return &Divergence{Report: report, Fingerprint: r.plan.Fingerprint, Scenario: scenario.String()}
}

// Divergence reports that the implementation does not behave as the
// specification says.
type Divergence struct {
	Report      string
	Fingerprint string
	Scenario    string
}

func (d *Divergence) Error() string { return d.Report }

// Check runs every plan test against fresh adapters from factory and returns
// the first failure: a *Divergence, or the adapter's own error.
func Check(spec string, factory func() Adapter, options ...Option) error {
	plan, err := LoadPlan(spec, options...)
	if err != nil {
		return err
	}
	runner, err := newRunner(plan)
	if err != nil {
		return err
	}
	for _, test := range plan.Tests {
		if err := runner.check(test, factory); err != nil {
			return err
		}
	}
	return nil
}

// Verify runs every plan test as a subtest of t.
func Verify(t *testing.T, spec string, factory func() Adapter, options ...Option) {
	t.Helper()
	plan, err := LoadPlan(spec, options...)
	if err != nil {
		t.Fatal(err)
	}
	runner, err := newRunner(plan)
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range plan.Tests {
		t.Run(test.Name, func(t *testing.T) {
			if err := runner.check(test, factory); err != nil {
				t.Fatal(err)
			}
		})
	}
}
