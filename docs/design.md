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
- `always` / `never` are checked as states are discovered and stop
  exploration at the first violation.
- `always possible` is backward reachability from the goal states.
- `eventually q` and `whenever p eventually q` reduce to: is there, from the
  initial state (or from a `p` state), a maximal path that stays outside `q`?
  Such a path either ends in a state with nothing enabled, or enters a cycle.
  Cycles are found as strongly connected components of the not-`q` subgraph
  (iterative Tarjan). A component is a counterexample if it is **fair**: for
  every fair call, the call is disabled in some state of the component or
  taken on an edge inside it. The reported loop visits what is needed to
  justify fairness.
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

## What is not claimed

- **Not a proof about code.** Conformance is exhaustive over the model's
  states and calls. It says nothing about inputs outside the modeled ranges
  or state the model omits.
- **Finite state only.** Bounded integers, booleans, enums, maps over them.
  No unbounded data, time, probability or floating point. Explicit-state
  exploration is practical to roughly millions of states; there is no
  symbolic engine, symmetry reduction or partial-order reduction.
- **Deterministic actions.** Nondeterminism is expressed through parameters.
- **Sequential conformance.** Interleavings are explored in the model;
  implementations are driven one call at a time. `zin trace` checks recorded
  concurrent runs after the fact if the log is linearized.
- **Weak fairness only.** No strong fairness, no general temporal logic; the
  property forms are fixed and each has a plain-English reading.

## Roadmap

- Bindings: JUnit 5, xUnit/.NET, RSpec, PHPUnit. Each is a plan replayer.
- A pytest plugin and a Vitest reporter that show model coverage.
- Strong fairness; symmetry reduction for maps keyed by interchangeable ids.
- Composition: several `system`s with shared actions.
- Refinement checking between two specifications.
- Published packages (crates.io, PyPI, npm) and prebuilt binaries.
