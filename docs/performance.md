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
time, median of per-process peak resident memory, and deterministic work counts
for that workload. Wall time includes process startup, reading inputs and
writing output. Check results also expose `calls`, `attempted_calls` and
`relabelings` in `check/1`. A completed exploration attempts every enumerated
call in each explored state.

## Measurements

Measured on 22 November 2026 on an Apple M4 Pro, macOS 27 arm64, with Rust 1.95
release builds of `3b8afd5` and the 0.3 development tree. Five timed samples
per binary. For small workloads, process startup accounts for much of the time.

| Workload | Baseline ms | 0.3 ms | Baseline peak MiB | 0.3 peak MiB | Work performed |
| --- | ---: | ---: | ---: | ---: | --- |
| Eight-worker mutex, reduction on | 23.53 | 23.14 | 15.78 | 15.97 | 17 representatives, 108 edges. The 0.3 run attempts 408 calls and constructs 40,320 relabelings. |
| Same mutex, reduction off | 13.37 | 6.21 | 16.31 | 3.36 | 1,280 states, 6,656 edges. The 0.3 run attempts 30,720 calls and constructs zero relabelings. |
| One-way counter, state coverage | 9.62 | 3.06 | 10.39 | 2.86 | Both reach all 401 states. Replay falls from 60,300 steps in 201 tests to 400 steps in one test. |
| One enabled call per nonterminal state | 3.14 | 3.08 | 3.42 | 3.61 | Both plans exercise 4,096 refusals. Accepted replay steps fall from 733 to 608. |
| Strongly fair retry | 2.49 | 2.50 | 2.25 | 2.42 | Three states and three edges, including a loop with an intermittently enabled fair action. |
| Recorded editor session | 88.52 | 35.73 | 8.00 | 2.83 | 120,000 events, 3,320,000 input bytes, including observed state after each server response. |
| Mutation audit | 37.49 | 24.07 | 43.55 | 4.27 | Seventy actions yield more than 500 candidates. Both runs check the first 500 and report 333 surviving mutants. |

Use step counts and graph coverage as regression assertions. Timing and memory
comparisons are most informative on the same machine and toolchain. Symmetry
remains expensive on the eight-worker example even though it reduces the graph
size. A different canonicalization algorithm needs an independent differential
comparison against full exploration.
