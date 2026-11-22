# Performance workloads

Build release binaries, then run the standard-library Python harness on macOS or
Linux. To compare two builds, supply a unique label for each executable.

```sh
cargo build --release --locked --target-dir target
python3 scripts/bench.py candidate=target/release/zin
python3 scripts/bench.py baseline=/path/to/old/zin candidate=target/release/zin --runs 5
```

The harness creates its models and event log in a temporary directory. Each
workload gets one discarded warm-up, then five fresh CLI processes per binary
by default. Binary order alternates. Each JSONL record contains the median wall
time, the median of per-process peak resident memory, and deterministic work
counts for that workload. Wall time includes process startup, reading inputs
and writing output.

## Measurements

Measured on 22 November 2026 on an Apple M4 Pro, macOS 27 arm64, with Rust 1.99.1
release builds of `3b8afd5` and this change. Five timed samples per binary. For
small workloads, process startup accounts for much of the time.

| Workload | Baseline ms | Now ms | Baseline peak MiB | Now peak MiB | Work performed |
| --- | ---: | ---: | ---: | ---: | --- |
| Recorded editor session | 88.75 | 35.49 | 8.08 | 2.69 | 120,000 events, 3,320,000 input bytes, observed state after each server response. |
| Mutation audit | 39.06 | 23.96 | 43.52 | 4.11 | Seventy actions yield more than 500 candidates. Both check the first 500 and report 333 surviving mutants. |
| Eight-worker mutex, reduction on | 40.23 | 39.17 | 15.86 | 15.84 | 17 representatives, 108 edges. |
| Same mutex, reduction off | 13.93 | 13.83 | 16.41 | 16.42 | 1,280 states, 6,656 edges. Relabeling tables are still built. |
| One-way counter, state coverage | 9.75 | 9.64 | 10.39 | 10.44 | 401 states, replayed as 60,300 steps in 201 tests. |
| One enabled call per nonterminal state | 3.47 | 3.38 | 3.39 | 3.44 | 4,096 refusals, 733 accepted steps in 21 tests. |
| Strongly fair retry | 2.98 | 3.27 | 2.33 | 2.31 | Three states, three edges. |

Trace replay and the audit changed in this revision. The other rows are
baselines for planner and symmetry work: the counter plan replays the same
prefixes over and over, and the mutex with reduction off pays for relabeling
tables it never uses.

Use step counts and graph coverage as regression assertions. Timing and memory
comparisons are most informative on the same machine and toolchain.
