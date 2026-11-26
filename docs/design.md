# Design

## The problem

A model checker proves things about a model. Teams ship implementations. The
usual bridge is a person reading the specification and writing tests by hand,
which is where formal methods lose most engineers: the proof and the code
drift apart, and the specification becomes a document.

An earlier plan for Zin (pull request #1) bridged the gap with a sidecar
process speaking JSON-RPC to the system under test and replaying scenarios.
It was dropped because it tests only the examples someone thought to write,
and it puts a new runner and protocol next to the test framework a team
already has, rather than inside it.

## The approach

**The proven model is the test oracle, and the tests are derived, not
written.** Once the reachable state graph is known, "what should the
implementation do with call *c* in state *s*?" has an exact answer for every
*s* and *c*. Zin walks the graph and emits call sequences that ask all of
those questions.

**The plan is data.** The checker writes one JSON document
([plan/1](plan-format.md)); a binding only replays it. That keeps the hard
part (parsing, type checking, exploration, tour generation) in one
implementation, makes every binding small enough to audit in one sitting, and
means the same specification produces the same tests in every language.

**Bindings live inside the test framework.** Tests are registered through the
framework's own mechanisms (`unittest.TestCase`, `describe`/`test`,
`t.Run`), so filtering, parallel runners, reporters, IDE integration and CI
all work unchanged. There is no second runner.

**The specification is tested too.** A model that permits everything passes
every check. `sometimes` guards against vacuity, and `zin audit` measures how
much of the specification is actually pinned down by its properties and
scenarios, using the same idea as mutation testing of code.

## Checking

- States are vectors of integers; enum values and booleans are interned.
  Exploration is breadth-first, so every safety counterexample is shortest.
- **Compound values are slot ranges.** A record is its members' slots in
  order, a map one value per key, a set one boolean per element, an
  optional a presence slot and the value, a list a length and every item it
  could hold, an enumeration with members a tag and each case's members.
  Compound expressions compile to one expression per slot, so exploration
  still evaluates integers only. Every value has one encoding: each write
  puts the first value of its slot into the slots an absent optional, the
  items past a list's length, or the cases not taken would hold, and
  symmetry images reset them the same way. Equality ignores those slots, so
  values computed in an expression need not be canonical.
- `always` / `never` are checked as states are discovered and stop
  exploration at the first violation.
- **Induction** (`check --inductive`) enumerates every well-typed state
  instead of the reachable ones: the product of each field's values, each in
  its one encoding. From each state where every `always` and `never` holds,
  every enabled call must lead to such a state again, without a fault. Since
  the reachable states already passed, a counterexample is always an
  unreachable state.
- **Symmetry.** Values of an `interchangeable` type cannot be named, so
  renaming them maps transitions to transitions and preserves every
  property. Exploration stores only the lexicographically smallest renaming
  of each state, found by trying every renaming (at most 8! = 40,320).
  Safety, deadlock, `sometimes`, `always possible` and unfair liveness are
  all invariant under renaming, so the reduced graph decides them exactly.
  A counterexample on representatives is lifted back: each step finds a
  renaming that takes the representative to the concrete state so far and
  takes the renamed call; a loop that comes back to a renaming of its start
  is unrolled until a concrete state recurs. Per-call fairness is not
  invariant (a representative's edge stands for different calls along a
  run), so liveness under fairness is checked on the full graph.
- `always possible` is backward reachability from the goal states.
- `eventually q` and `whenever p eventually q` reduce to: is there, from the
  initial state (or from a `p` state), a maximal path that stays outside `q`?
  Such a path either ends in a state with nothing enabled, or enters a cycle.
  Cycles are found as strongly connected components of the not-`q` subgraph
  (iterative Tarjan). A component is a counterexample if it is **fair**: for
  every weakly fair call, the call is disabled in some state of the component
  or taken on an edge inside it, and every strongly fair call enabled in the
  component is taken inside it. A component that fails only the strong
  condition may still hold a fair loop that avoids the states enabling the
  starved calls, so those states are removed and the rest decomposed again
  (Emerson–Lei); each round removes states, and a surviving component's set
  of enabled strongly fair calls shrinks, so the rounds are few. The reported
  loop visits what is needed to justify fairness.
- Everything recursive over input is bounded (nesting depth, state count,
  transitions, calls), and limits fail loudly.

## Tour generation

The plan must cover a set of obligations: transitions to take and (state,
call) refusals to try. Refusals are free once the tour is in the right state,
since a refused call does not move the model. A tour greedily discharges
everything available in the current state, then follows the shortest path to
the nearest state with something left; when nothing is reachable, or the step
cap is hit, a new tour starts from the initial state (a fresh
implementation). This is a heuristic for the rural postman problem: not
minimal, but linear-ish in practice and deterministic.

## Composition

A file with several `system` blocks is merged into one specification before
type checking: fields side by side, an action with the same name in several
blocks merged into one (requirements and assignments concatenated, same-named
parameters unified), everything else carried over. This is the synchronized
product of CSP-style processes, and since the result is an ordinary
specification, every checker, the planner and the bindings work unchanged.
The merge enforces encapsulation that one flat block cannot express: a
block's actions and initial values may touch only its own fields, so the
shared actions are the only coupling between components.

## What is not claimed

- **Not a proof about code.** Conformance is exhaustive over the model's
  states and calls. It says nothing about inputs outside the modeled ranges
  or state the model omits. Plans are derived from the full state space even
  when the specification declares interchangeable values.
- **Finite state only.** Bounded integers, booleans and enums, and records,
  maps, optional values, sets, bounded lists and enumerations with members
  built from them.
  No unbounded data, time, probability or floating point. Explicit-state
  exploration is practical to roughly millions of states; there is no
  symbolic engine or partial-order reduction, and symmetry reduction covers
  only fully interchangeable values.
- **Deterministic actions.** Nondeterminism is expressed through parameters.
- **Sequential conformance.** Interleavings are explored in the model;
  implementations are driven one call at a time. `zin trace` checks recorded
  concurrent runs after the fact if the log is linearized.
- **Per-call fairness only.** Weak and strong fairness per call, no general
  temporal logic; the property forms are fixed and each has a plain-English
  reading.

## Roadmap

- Bindings: JUnit 5, xUnit/.NET, RSpec, PHPUnit. Each is a plan replayer.
- A pytest plugin and a Vitest reporter that show model coverage.
- Composition across files, and plans that test one component against the
  rest of the composition as its environment.
- Refinement checking between two specifications.
