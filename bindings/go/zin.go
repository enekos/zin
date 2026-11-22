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
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"sort"
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

// Plan is a derived test plan, as written by `zin plan`. docs/plan.schema.json
// defines it; the pointer fields are absent from plans written before 0.3.
type Plan struct {
	Format      string            `json:"zin"`
	System      string            `json:"system"`
	Fingerprint string            `json:"fingerprint"`
	Sources     map[string]string `json:"sources"`
	Component   *string           `json:"component"`
	Cover       string            `json:"cover"`
	MaxSteps    *int              `json:"max_steps"`
	StepBudget  *int              `json:"step_budget"`
	PairLimit   *int              `json:"pair_limit"`
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
	States        int  `json:"states"`
	Transitions   int  `json:"transitions"`
	Refusals      *int `json:"refusals"`
	EligiblePairs *int `json:"eligible_pairs"`
	CoveredPairs  *int `json:"covered_pairs"`
	Tests         int  `json:"tests"`
	AcceptedSteps int  `json:"accepted_steps"`
	RejectedSteps int  `json:"rejected_steps"`
}

type config struct {
	cover      *string
	maxSteps   *int
	stepBudget *int
	pairLimit  *int
	component  *string
	plan       string
	binary     string
	invalid    []string
}

// Option adjusts how the plan is obtained. Only the planning options passed
// are compared with a committed plan's recorded ones.
type Option func(*config)

// WithCover selects the coverage goal: transitions, states, actions,
// scenarios or pairs. Without it, `zin plan` uses transitions.
func WithCover(cover string) Option { return func(c *config) { c.cover = &cover } }

func (c *config) positive(name string, value int) *int {
	if value < 1 {
		c.invalid = append(c.invalid, fmt.Sprintf("%s must be a positive integer, not %d", name, value))
	}
	return &value
}

// WithMaxSteps sets the soft target for tour length (`zin plan --max-steps`).
func WithMaxSteps(steps int) Option {
	return func(c *config) { c.maxSteps = c.positive("max steps", steps) }
}

// WithStepBudget sets the hard limit on all replay steps (`zin plan --step-budget`).
func WithStepBudget(steps int) Option {
	return func(c *config) { c.stepBudget = c.positive("step budget", steps) }
}

// WithPairLimit samples this many adjacent-call pairs with WithCover("pairs").
func WithPairLimit(pairs int) Option {
	return func(c *config) { c.pairLimit = c.positive("pair limit", pairs) }
}

// WithComponent tests one block of a composition in isolation (`zin plan --component`).
func WithComponent(name string) Option { return func(c *config) { c.component = &name } }

// WithPlan uses a pre-derived plan file instead of running `zin plan`.
func WithPlan(path string) Option { return func(c *config) { c.plan = path } }

// WithBinary sets the path of the zin binary.
func WithBinary(path string) Option { return func(c *config) { c.binary = path } }

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
		return nil, fmt.Errorf("zin: cannot read plan %s: %w", path, err)
	}
	return parsePlan(data, path)
}

func parsePlan(data []byte, origin string) (*Plan, error) {
	var format struct {
		Format string `json:"zin"`
	}
	if json.Unmarshal(data, &format) != nil || format.Format != PlanFormat {
		return nil, fmt.Errorf("zin: %s is not a %s file", origin, PlanFormat)
	}
	var plan Plan
	if err := json.Unmarshal(data, &plan); err != nil {
		return nil, fmt.Errorf("zin: the plan is malformed: %s: %w", origin, err)
	}
	if err := validate(&plan); err != nil {
		return nil, err
	}
	return &plan, nil
}

func validate(plan *Plan) error {
	params := make(map[string]int, len(plan.Actions))
	for _, action := range plan.Actions {
		params[action.Name] = len(action.Params)
	}
	for index, call := range plan.Calls {
		want, known := params[call.Action]
		if !known {
			return fmt.Errorf("zin: the plan is malformed: call %d (%s) uses the unknown action %s", index, call.Text, call.Action)
		}
		if len(call.Args) != want {
			return fmt.Errorf("zin: the plan is malformed: call %d (%s) has %d arguments, but %s takes %d",
				index, call.Text, len(call.Args), call.Action, want)
		}
	}
	if len(plan.States) == 0 || len(plan.Parents) != len(plan.States) {
		return errors.New("zin: the plan is malformed: states and parents do not line up")
	}
	for _, test := range plan.Tests {
		for _, step := range test.Steps {
			if step[0] < 0 || step[0] >= len(plan.Calls) || step[1] < -1 || step[1] >= len(plan.States) {
				return fmt.Errorf("zin: the plan is malformed: step %v is out of range", step)
			}
		}
	}
	return nil
}

// currentFingerprint is the fingerprint `zin plan` would record now, or ""
// if a file is unreadable. With included files, the entry file's hash and
// each source's current hash, sorted by path, are hashed together (see
// docs/plan-format.md).
func currentFingerprint(spec string, sources map[string]string) string {
	source, err := os.ReadFile(spec)
	if err != nil {
		return ""
	}
	entry := Fingerprint(source)
	if len(sources) == 0 {
		return entry
	}
	paths := make([]string, 0, len(sources))
	for path := range sources {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	var text strings.Builder
	text.WriteString(entry + "\n")
	for _, path := range paths {
		included, err := os.ReadFile(filepath.Join(filepath.Dir(spec), filepath.FromSlash(path)))
		if err != nil {
			return ""
		}
		text.WriteString(path + "\t" + Fingerprint(included) + "\n")
	}
	return Fingerprint([]byte(text.String()))
}

func fresh(plan *Plan, spec string) bool {
	return currentFingerprint(spec, plan.Sources) == plan.Fingerprint
}

func equalInt(recorded, requested *int) bool {
	return requested == nil || (recorded != nil && *recorded == *requested)
}

// compatible compares only the options passed; the component is identity.
func compatible(plan *Plan, settings config) bool {
	recorded, requested := "", ""
	if plan.Component != nil {
		recorded = *plan.Component
	}
	if settings.component != nil {
		requested = *settings.component
	}
	return recorded == requested &&
		(settings.cover == nil || *settings.cover == plan.Cover) &&
		equalInt(plan.MaxSteps, settings.maxSteps) &&
		equalInt(plan.StepBudget, settings.stepBudget) &&
		equalInt(plan.PairLimit, settings.pairLimit)
}

func (c config) arguments(spec string) []string {
	args := []string{"plan", spec}
	if c.cover != nil {
		args = append(args, "--cover", *c.cover)
	}
	for _, option := range []struct {
		flag  string
		value *int
	}{{"--max-steps", c.maxSteps}, {"--step-budget", c.stepBudget}, {"--pair-limit", c.pairLimit}} {
		if option.value != nil {
			args = append(args, option.flag, fmt.Sprint(*option.value))
		}
	}
	if c.component != nil {
		args = append(args, "--component", *c.component)
	}
	return args
}

func (c config) any() bool {
	return c.cover != nil || c.maxSteps != nil || c.stepBudget != nil || c.pairLimit != nil || c.component != nil
}

// LoadPlan returns the plan for spec, following the runner contract in
// docs/plan-format.md: the WithPlan file when given, else a sibling
// <name>.plan.json, as long as it is fresh and was derived with every
// planning option passed here; otherwise the output of `zin plan`
// (WithBinary, $ZIN_BIN, or zin on PATH). spec may itself be a plan file.
func LoadPlan(spec string, options ...Option) (*Plan, error) {
	var settings config
	for _, option := range options {
		option(&settings)
	}
	if len(settings.invalid) > 0 {
		return nil, errors.New("zin: " + settings.invalid[0])
	}
	if strings.HasSuffix(spec, ".json") {
		if settings.any() || settings.plan != "" {
			return nil, errors.New("zin: planning options need a .zin specification, not a plan file")
		}
		plan, err := readPlan(spec)
		if err != nil {
			return nil, err
		}
		if strings.HasSuffix(spec, ".plan.json") {
			source := strings.TrimSuffix(spec, ".plan.json") + ".zin"
			if _, err := os.Stat(source); err == nil && !fresh(plan, source) {
				return nil, fmt.Errorf("zin: %s was derived from a different version of %s; regenerate it with: zin plan %s --out %s",
					spec, filepath.Base(source), source, spec)
			}
		}
		return plan, nil
	}
	if info, err := os.Stat(spec); err != nil || info.IsDir() {
		return nil, fmt.Errorf("zin: cannot read specification %s", spec)
	}
	args := settings.arguments(spec)
	sibling := strings.TrimSuffix(spec, filepath.Ext(spec)) + ".plan.json"
	cached := sibling
	if settings.plan != "" {
		cached = settings.plan
	}
	regenerate := "zin " + strings.Join(append(append([]string{}, args...), "--out", cached), " ")
	reason := ""
	if _, err := os.Stat(cached); settings.plan != "" || err == nil {
		plan, err := readPlan(cached)
		if err != nil {
			return nil, err
		}
		switch {
		case !fresh(plan, spec):
			reason = fmt.Sprintf("%s was derived from a different version of %s", cached, filepath.Base(spec))
		case !compatible(plan, settings):
			reason = fmt.Sprintf("%s was derived with different planning options", cached)
		default:
			return plan, nil
		}
		if settings.plan != "" {
			return nil, fmt.Errorf("zin: %s; regenerate it with: %s", reason, regenerate)
		}
	}
	prefix := ""
	if reason != "" {
		prefix = reason + "; "
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
		return nil, fmt.Errorf("zin: %sthe zin binary was not found. Install zin-spec, set ZIN_BIN, or regenerate the plan: %s",
			prefix, regenerate)
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
		return int64(reflected.Uint())
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
	if err := validate(plan); err != nil {
		return nil, err
	}
	params := make(map[string][]string, len(plan.Actions))
	for _, action := range plan.Actions {
		names := make([]string, 0, len(action.Params))
		for _, param := range action.Params {
			names = append(names, param.Name)
		}
		params[action.Name] = names
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
			args[names[position]] = plain(value)
		}
		err := adapter.Call(call.Action, args)
		if err != nil && !errors.Is(err, ErrRejected) {
			return index + 1, "", &adapterError{call: call.Text, err: err, steps: r.listing(steps[:index+1])}
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

// adapterError is an error the adapter returned that is not a rejection.
type adapterError struct {
	call  string
	err   error
	steps string
}

func (e *adapterError) Error() string {
	return fmt.Sprintf("%s failed: %v\nsteps taken:\n%s", e.call, e.err, e.steps)
}

func (e *adapterError) Unwrap() error { return e.err }

func (r *runner) check(test Test, factory func() Adapter) error {
	failed, message, err := r.run(factory(), test.Steps)
	if err != nil {
		return err
	}
	if message == "" {
		return nil
	}
	taken := test.Steps[:failed]
	lines := make([]string, 0, len(taken))
	for _, step := range taken {
		lines = append(lines, "  "+r.text(step))
	}
	if len(lines) == 0 {
		lines = append(lines, "  expect true")
	}
	scenario := "scenario \"Replay divergence\"\n" + strings.Join(lines, "\n")
	report := fmt.Sprintf("%s: implementation diverges from the specification in test \"%s\" at step %d\n%s\nsteps taken:\n%s\nspec fingerprint: %s\n%s",
		r.plan.System, test.Name, failed, message, r.listing(taken), r.plan.Fingerprint, scenario)
	if failed > 0 {
		if short := r.shortest(test.Steps, failed); len(short) < len(taken) {
			again, repeated, err := r.run(factory(), short)
			var raised *adapterError
			switch {
			case errors.As(err, &raised):
				report += "\nthe shorter replay raised: " + raised.err.Error()
			case err != nil:
				report += "\nthe shorter replay raised: " + err.Error()
			case again == len(short) && repeated == message:
				report += "\nshortest reproduction:\n" + r.listing(short)
			default:
				report += "\nthe shortest path to the same specification state does not reproduce the original mismatch"
			}
		}
	}
	return &Divergence{Report: report, Fingerprint: r.plan.Fingerprint, Scenario: scenario}
}

// Divergence reports that the implementation does not behave as the
// specification says.
type Divergence struct {
	Report string
	// Fingerprint identifies the specification the plan was derived from.
	Fingerprint string
	// Scenario is the original failing steps as a Zin scenario, ready to paste.
	Scenario string
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
