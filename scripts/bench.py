#!/usr/bin/env python3
"""Measure release binaries on reproducible model checking and replay workloads."""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]


def workloads(directory):
    def write(name, text):
        path = directory / name
        path.write_text(text, encoding="utf-8")
        return str(path)

    mutex = (ROOT / "examples/mutex.zin").read_text()
    mutex_path = write("mutex.zin", mutex.replace(
        "ann, ben, cid, dot", "ann, ben, cid, dot, eve, fay, gia, hal"
    ))
    counter = write("counter.zin", '''system "Counter"
state
  value is an integer from 0 to 400
initially
  value is 0
action increment
  requires value < 400
  value becomes value + 1
''')
    sparse = write("sparse.zin", '''system "One enabled call per state"
state
  current is an integer from 0 to 64
initially
  current is 0
action advance(n is an integer from 1 to 64)
  requires n is current + 1
  current becomes n
''')
    retry = write("retry.zin", '''system "Retry under a fair scheduler"
state
  phase is one of idle, running, done
initially
  phase is idle
eventually phase is done
action start
  requires phase is idle
  phase becomes running
action pause
  requires phase is running
  phase becomes idle
strongly fair action finish
  requires phase is running
  phase becomes done
''')
    editor = str(ROOT / "examples/editor.zin")
    # Each round saves 1 and then 0, returning to exactly the initial state.
    round_trip = [
        {"action": "increase"}, {"action": "save"},
        {"action": "server_ok", "state": {"saved": 1}},
        {"action": "decrease"}, {"action": "save"},
        {"action": "server_ok", "state": {"saved": 0}},
    ]
    lines = "".join(json.dumps(event, separators=(",", ":")) + "\n" for event in round_trip)
    events = write("editor.jsonl", lines * 20_000)
    audit = write("mutations.zin",
        'system "Mutation cap"\nstate\n  value is an integer from 0 to 0\ninitially\n  value is 0\n'
        + "".join(f"action op{i}\n  requires value is 0 and value is 0\n  value becomes value\n"
                  for i in range(70))
    )
    return [
        ("mutex-on", ["check", mutex_path, "--symmetry", "on", "--format", "json"]),
        ("mutex-off", ["check", mutex_path, "--symmetry", "off", "--format", "json"]),
        ("counter-plan", ["plan", counter, "--cover", "states"]),
        ("refusal-plan", ["plan", sparse, "--cover", "transitions"]),
        ("strong-fairness", ["check", retry, "--format", "json"]),
        ("editor-trace", ["trace", editor, events]),
        ("audit-cap", ["audit", audit]),
    ]


def measure(binary, case, args, directory):
    # wait4 returns this child's rusage. RUSAGE_CHILDREN would include the
    # highest previous child in the session, giving later cases a false peak.
    with tempfile.TemporaryFile(dir=directory) as output, tempfile.TemporaryFile(dir=directory) as errors:
        start = time.perf_counter_ns()
        with subprocess.Popen([binary, *args], stdout=output, stderr=errors) as process:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        milliseconds = (time.perf_counter_ns() - start) / 1_000_000
        if process.returncode != (1 if case == "audit-cap" else 0):
            errors.seek(0)
            raise RuntimeError(f"{case} exited {process.returncode}: {errors.read().decode()}")
        size = output.tell()
        output.seek(0)
        data = output.read()
    # Darwin reports bytes; Linux reports KiB.
    peak_mib = usage.ru_maxrss / (1024 ** 2 if sys.platform == "darwin" else 1024)
    metrics = {}
    if args[0] == "check":
        result = json.loads(data)["results"][0]
        for key in ("states", "transitions", "attempted_calls", "relabelings"):
            if key in result:
                metrics[key] = result[key]
    elif args[0] == "plan":
        summary = json.loads(data)["summary"]
        metrics = {key: summary[key] for key in ("states", "transitions", "tests", "accepted_steps", "rejected_steps")}
        metrics["plan_bytes"] = size
    elif args[0] == "trace":
        assert "120000 events in 1 run(s)" in data.decode()
        metrics = {"events": 120_000, "input_bytes": os.path.getsize(args[2])}
    else:
        text = data.decode()
        assert "only the first 500 mutants were tried" in text
        metrics = {"mutants": 500, "survivors": len(re.findall(r"^SURVIVED", text, re.MULTILINE))}
    return milliseconds, peak_mib, metrics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="+", help="One or more uniquely labeled LABEL=/path/to/zin")
    parser.add_argument("--runs", type=int, default=5, help="Timed runs after one warmup (default 5)")
    options = parser.parse_args()
    if options.runs < 1 or any("=" not in value for value in options.binary):
        parser.error("use a positive run count and LABEL=/path/to/zin for each binary")
    binaries = [(label, str(Path(path).resolve())) for label, path in (v.split("=", 1) for v in options.binary)]
    if len({label for label, _ in binaries}) != len(binaries):
        parser.error("binary labels must be unique")
    if sys.platform != "darwin" and not sys.platform.startswith("linux"):
        parser.error("this harness measures per-process RSS on macOS and Linux")
    with tempfile.TemporaryDirectory(prefix="zin-bench-") as tmp:
        directory = Path(tmp)
        for case, args in workloads(directory):
            measurements = {label: [] for label, _ in binaries}
            for index in range(options.runs + 1):
                # Alternate order between comparison binaries.
                for label, binary in (binaries if index % 2 == 0 else reversed(binaries)):
                    sample = measure(binary, case, args, directory)
                    if index:
                        measurements[label].append(sample)
            for label, points in measurements.items():
                counts = points[0][2]
                if not all(p[2] == counts for p in points):
                    raise RuntimeError(f"{case} produced different work counts across runs")
                print(json.dumps({
                    "case": case, "binary": label, "runs": options.runs,
                    "platform": platform.system(), "architecture": platform.machine(),
                    "wall_ms": round(statistics.median(p[0] for p in points), 2),
                    "peak_mib": round(statistics.median(p[1] for p in points), 2),
                    **counts,
                }), flush=True)


if __name__ == "__main__":
    main()
