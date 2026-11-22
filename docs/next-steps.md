# Zin development plan

**Draft · 22 November 2026 · reviewed against `main` at `3b8afd5`.**

Fix the checker verdict and the contract between checker and runners before releasing 0.3.0. Judge the next development cycle by the implementation defects Zin catches, the cost of replay, and whether a developer can turn each failure into a regression test in their own repository.

## Release blockers

### 1. Default checking can miss a reachable model fault

With the specification below, `zin check workers.zin` exits 0 after exploring two representatives. `zin check workers.zin --symmetry off` exits 1 with a division-by-zero model fault on line 8 after `start(ann)`. The two workers are interchangeable. In one concrete state the first division faults. In its representative the first worker satisfies the predicate and `some` stops evaluating. Renaming preserves the mathematical Boolean condition only when evaluation also preserves definedness. [Evaluator](../src/model.rs) · [Symmetry reduction](../src/symmetry.rs)

```zin
system "Two workers"
types
  interchangeable worker is one of ann, ben
state
  load is a map from worker to an integer from 0 to 1
initially
  load of every worker is 0
always some w in worker satisfies 1 / (1 - load of w) > 0
action start(w is a worker)
  requires every v in worker satisfies load of v is 0
  load of w becomes 1
```

**First change.** Preserve the existing concrete evaluation semantics. Fall back to full exploration wherever reduction cannot establish that evaluation is invariant under renaming, and say why in the check result. Disabling reduction by default is a viable interim fix. Include guards, assignment expressions, map keys, terminal conditions and properties in the eligibility decision. Test overflow and out-of-range map access alongside division. Until fixed, use `--symmetry off` for verification of affected models.

**Acceptance.** Both modes report a model fault on this fixture. A generated suite of small, well-typed models compares completed verdicts against full exploration and replays every reported trace on concrete states. Include short-circuiting, partial expressions and enum declaration reorderings. Preserve the existing fairness oracle and mutex reduction tests. Treat exploration-limit results separately from completed verdicts. [Existing symmetry tests](../tests/symmetry.rs) · [Fairness oracle](../src/properties.rs)

### 2. The public Rust planner accepts a reduced graph

`Plan::build(&verify_with(&spec, Options::default())?, …)` succeeds on `examples/mutex.zin` with 9 representatives. Replaying the resulting plan against its 48 concrete states disagrees on the expected state after the first scenario call, `want(ann)`. The CLI uses full verification and takes the correct path. A library caller can readily assemble the invalid combination from public functions. [Verification API](../src/check.rs) · [Plan builder](../src/plan.rs) · [Mutex fixture](../examples/mutex.zin)

**First change.** Reject reduced graphs at plan construction with a diagnostic pointing to `verify`. Use the graph's own reduction status. Consider a distinct full-verification type when revising the public API. Check other public consumers that promise concrete states. **Acceptance.** The reduced call returns a typed error, the full call succeeds, and every emitted call, outcome and destination replays on the full graph.

### 3. Make coverage and adapter behavior dependable

| Reproduced on the reviewed commit | Change and acceptance test |
| --- | --- |
| A switch with two states, guarded `start` and `stop`, and no scenarios produces one acceptance and **zero refusals** under `--cover states`. The guide promises every refusal. Default `transitions` exercises both acceptances and both refusals. | Keep `states` affordable by specifying state visits as its guarantee. Correct the guide and release notes, direct users needing refusal coverage to `transitions`, and test unique obligations for every mode. Also settle whether `scenarios` includes generated `sometimes` witnesses, as the builder currently does. |
| A Rust adapter observing one correct entry in a two-entry map fails before the first action. Partial map observations are documented and the other replayers implement recursive subset comparison. | Make Rust compare only supplied entries, while rejecting unknown keys, wrong values and wrong types. Put these cases in one fixture set exercised by Rust, Python, JS, Go and trace replay where applicable. |
| The editor's longest generated tour is 43 at `--max-steps 40`. With its default sibling plan present, JS `loadPlan(…, {maxSteps: 40})` returns a tour of 198 steps. | Specify the soft tour target and distinguish it from a total replay budget. Include plan-affecting options in cache compatibility. Re-derive or explain how to regenerate an incompatible committed plan. Test fresh, cached and binary-free use, including old `plan/1` files. |
| `zin init pytest` accepts an action named `class` and emits `def class(self):`, which Python cannot compile. | Support an explicit action-name mapping or give a precise scaffold diagnostic. Check framework lifecycle names and compile or collect generated skeletons in their native runners. |

[Coverage and adapter guide](conformance.md) · [Plan builder](../src/plan.rs) · [Rust adapter](../src/conform.rs) · [JS plan loader](../bindings/js/index.cjs) · [Scaffolds](../src/scaffold.rs)

### Make the language reference executable

Pair each rule that affects a verdict with a small accepted and rejected example. Start with comparison precedence, quantifier scope, simultaneous writes, map defaults and overrides, short-circuit evaluation, fairness per call, and shared parameters in composed actions. Run the examples through the public parser and checker. For faults, assert the diagnostic kind, source location and a replayable prefix. Add equivalent spellings and harmless renamings as test transformations.

Give diagnostics stable codes and file/column spans, starting with name resolution and type mismatches. Carry both declaration sites into conflicting composition errors. Keep a previous-release fixture set for specs and saved plans. Additive diagnostics can stay in `check/1`. A future feature that changes what a runner must assert, such as an action return value, needs an explicit compatibility decision so an older runner cannot silently skip the assertion. [Language reference](language.md) · [Parser](../src/parser.rs) · [Wire formats](plan-format.md)

## Find more defects in existing codebases

**Start with repeated calls.** I ran a Python adapter whose `ack` accepts the first acknowledgement and rejects the second. Its model has one state, one accepted self-loop and an unchanged observable field. The default transition plan passes after one call. A scenario containing `ack` twice fails at step two. The plan covered every model transition. The defect depended on the history by which the implementation reached that state. This is a small, reproducible reason to add a second coverage option.

1. **Build a paired implementation corpus.** Start with that acknowledgement fixture, a refusal that changes state, an off-by-one account and a stale save response in the editor. For each, keep a faulty and a corrected implementation behind the same adapter. Add one historical bug/fix pair from a real library or service with a resettable public API. Record detection by mode, unique mismatches, observed fields, replay calls, fresh setups and wall time. Keep specification mutation results in their own column. A green audit of a model answers a different question from catching a bug in code.
2. **Prototype bounded coverage of adjacent calls.** Reuse the existing graph and plan format to exercise feasible pairs of calls, including repeats, a rejection followed by another call, and two histories that converge on the same model state. Start with a deterministic subset under an explicit replay budget and report both covered and eligible pairs. Ship it when it catches the double acknowledgement automatically, replays the same failure every run, keeps the fixed adapter green, and shows the extra cost and incremental detections over default tours on the corpus. Use these results to decide whether longer seeded walks pay for themselves.
3. **Make failures reusable.** Emit the original failing prefix, spec fingerprint, expected and observed outcome or field, and a pasteable scenario or replayable JSONL fixture. Rerun shorter candidates on fresh adapters and require the same mismatch at the same operation. If a shorter path fails differently, retain the original. For surviving spec mutants, show the shortest distinguishing call sequence and the original and mutated behavior so the author can decide which rule to encode. [Runner reporting](../src/conform.rs) · [Audit](../src/audit.rs) · [Trace event format](conformance.md#checking-recorded-executions)
4. **Pilot the first hour of adoption.** Have a developer pick one public operation with a business rule, model its inputs and refusals, generate an adapter, expose a meaningful observation and replay a known regression. Run one service or library pilot and one browser workflow. Measure time to first useful failure, adapter work, and PR replay time. Report which fields were actually compared, including runs that only checked acceptance. Prototype return-value or emitted-event assertions if a pilot finds a wrong result while acceptance and observed state match.

## Performance work

Release binary on an Apple M4 Pro, macOS arm64, Rust 1.95.0. Mutex results are medians of five fresh CLI processes after one warm-up per variant. Elapsed time includes process launch and the timing wrapper. Memory is peak resident size reported by macOS. These workloads identify where to profile. Use deterministic work counts for PR gates and measure timing trends on a fixed runner.

| Workload | Result | Implication |
| --- | --- | --- |
| Shipped mutex expanded to eight workers, symmetry on | 17 states, 108 edges, 24.9 ms, 15.8 MiB | Representative count alone gives a misleading picture of cost. |
| Same model with `--symmetry off` | 1,280 states, 6,656 edges, 14.8 ms, 16.4 MiB | Compilation still builds the relabeling tables. |
| Same eight-worker model with the `interchangeable` annotation removed | 1,280 states, 6,656 edges, 7.2 ms, 3.2 MiB | Measure table construction separately and skip it on full-graph paths. |
| One-way counter from 0 to 400, `--cover states`, default target 200 | 60,300 accepted steps, 201 tests, 478,422 JSON bytes. Target 400 yields 400 steps, one test, 12,124 bytes. | Replaying the same prefixes adds 59,900 adapter calls. A 200-step hard cap cannot reach state 400 from a fresh instance. |

**Order of work**

1. Make these benchmarks reproducible in the repo. Generate the mutex by extending both four-worker enum lists in the shipped example. Generate the counter with a single `increment` guarded by `value < 400`. Add a refusal-heavy model, a fair-liveness model, and a recorded editor session. Track attempted calls, unique states, edges, relabelings, emitted steps, setup count, plan bytes, elapsed time and peak memory for each applicable phase.
2. Fix the planner's restart behavior first. Compare shortest access depth with the requested target before scheduling a fresh tour. When an obligation already needs a long setup, continue covering useful work from that state, or explain that a requested strict cap makes it unreachable. Bound **actual** emitted steps across scenarios, witnesses, routes and refusals. The current two-million-step check bounds the initial transition and refusal obligations in one mode. The regression test should visit all 401 counter states with at most 800 emitted steps at the default soft target, preserve exhaustive transition/refusal coverage, and report a limit when the total budget runs out. [Tour generation](../src/plan.rs)
3. Separate symmetry eligibility validation from table construction. Decide whether fairness requires full exploration before allocating the tables. Then profile canonicalization before changing its algorithm. Require full-check and plan paths to build zero relabeling tables, compare completed verdicts with concrete exploration, and record wall time and peak memory before and after. [Compiler](../src/model.rs) · [Relabeling tables](../src/symmetry.rs)
4. Profile two additional costs before committing to a redesign. Audit clones a full spec per candidate before truncating to 500. Trace reads the whole log and formats the current state even on successful events. Try lazy mutant generation and line-by-line trace replay with diagnostic formatting on failure. Compare peak memory and throughput while preserving mutant ordering, trace line numbers, input limits and full model verification. [Audit candidates](../src/audit.rs) · [CLI trace path](../src/cli.rs) · [Trace replay](../src/trace.rs)

## Release and roadmap order

Strong fairness, same-file synchronized composition, symmetry reduction and the browser runner are on `main`. GitHub's latest release is 0.2.0. The crates index and npm both list Zin 0.2.0. PyPI returned 404 for `zin-verify` on the review date. The root README advertises a PyPI install and the JS binding README still describes npm as unpublished. Both the release tag and `main` fail `cargo check --locked` on their advertised Rust 1.85.0 minimum with unstable let-expression errors. Stable-only CI misses this. [Release](https://github.com/enekos/zin/releases/tag/v0.2.0) · [Crates index](https://index.crates.io/zi/n-/zin-spec) · [npm](https://registry.npmjs.org/zin-verify/latest) · [PyPI](https://pypi.org/pypi/zin-verify/json) · [CI](../.github/workflows/ci.yml)

| Sequence | Deliverable | Ready to move on when |
| --- | --- | --- |
| **First PRs** | Checker fallback and differential fixture, then the concrete-plan API guard. In parallel, fix coverage wording, map parity and cache handling. | Both reproduced verification failures have regression tests. Every supported runner agrees on the shared observation fixtures. |
| **0.3.0 release preparation** | Restore Rust 1.85 compatibility or declare and test the lowest supported compiler. Compile generated adapters, install the packaged artifacts in clean consumers, document the `Fairness` API migration and version registry-specific instructions. | CI tests the declared minimum and stable. Each advertised install path works with the released artifacts and a deliberately broken adapter fails in its native runner. Give checkout instructions for any unpublished binding. |
| **Next development cycle** | Paired corpus, planner budget and counter fix, then adjacent-call pilot and failure export. Start the executable language reference with the rules implicated by these fixes. | Known faulty/fixed pairs give the expected outcomes. A developer can reproduce the same failure from the emitted artifact. The 401-state planner regression meets its work budget. |
| **Coverage reporters** | Advance the existing pytest and Vitest roadmap items once the planner exposes denominators. Show unique planned, executed and failed obligations plus observed fields. | Filtering or aborting a run produces accurate partial coverage. Both reporters agree on a shared fixture. |
| **Bindings** | Pick JUnit 5 or xUnit/.NET when a pilot codebase can exercise it. Use the shared runner fixtures as the admission test. Keep the other, RSpec and PHPUnit demand-driven. | A real consumer installs the package, filters a generated test and replays a known failure. |
| **Composition and refinement** | Add multi-file composition after source locations and cache provenance can cover every input. Then pilot per-component plans with a controllable environment. Scope refinement last, starting with bounded deterministic models, an explicit mapping of states/actions, and a stated rule for allowed refusals. | Multi-file and flattened models agree. Editing an imported spec invalidates the plan. A component can be tested in isolation. A failed refinement yields a concrete distinguishing trace. |

The first two PRs address incorrect results independently and can be reviewed separately. For the product pilot, prioritize extra defects caught per replay minute and time to a reproducible failure. Those measurements provide the decision point for more language features and bindings. [Existing roadmap](design.md#roadmap)

## Verification baseline

On the reviewed SHA, `cargo fmt --all --check`, Clippy with warnings denied and all 53 Rust tests passed on Rust 1.95.0. All seven top-level example specs passed `check` and `audit`. Python passed 19 pytest tests and five unittest cases. Go passed vet and its tests. A clean, lockfile-based install passed the JS binding suite and the React example, including 16 browser binding tests and 11 example browser tests. The minimum-compiler run and the focused probes above expose gaps beyond those suites. The repeated-ack adapter and counter are synthetic. The historical-defect and onboarding pilots are proposed work. [CI run for the reviewed commit](https://github.com/enekos/zin/actions/runs/44938488159)
