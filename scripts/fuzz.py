#!/usr/bin/env python3
"""Run the app's random-input test over many seeds, a few at a time.

Each seed runs in its own process (the test aborts itself past
TONIQUE_FUZZ_MAX_MB, so a runaway seed can't take the machine down). Logs go
to target/fuzz/<seed>.log, and failures are summed up at the end.

    scripts/fuzz.py                  # seeds 0..99, 500 inputs each
    scripts/fuzz.py --seeds 200-299 --steps 1000 --jobs 2
"""

import argparse
import os
import re
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TEST = "ui::app::tests::random_input"


def seeds(text):
    start, _, end = text.partition("-")
    return range(int(start), int(end or start) + 1)


def test_binary():
    out = subprocess.run(
        ["cargo", "test", "--bin", "tonique_daw", "--no-run", "--message-format=json"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout
    found = re.findall(r'"executable":"([^"]+tonique_daw-[0-9a-f]+)"', out)
    if not found:
        sys.exit("couldn't find the test binary")
    return found[-1]


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--seeds", type=seeds, default=seeds("0-99"), help="e.g. 0-99")
    parser.add_argument("--steps", type=int, default=500, help="inputs per seed")
    parser.add_argument("--jobs", type=int, default=4, help="runs at once")
    args = parser.parse_args()

    binary = test_binary()
    logs = ROOT / "target" / "fuzz"
    logs.mkdir(parents=True, exist_ok=True)
    pending = list(args.seeds)
    running = {}  # seed -> (process, log file)
    results = {}  # seed -> "ok" | reason

    while pending or running:
        while pending and len(running) < args.jobs:
            seed = pending.pop(0)
            log = open(logs / f"{seed}.log", "w")
            env = dict(os.environ, TONIQUE_FUZZ_SEED=str(seed),
                       TONIQUE_FUZZ_STEPS=str(args.steps), TONIQUE_FUZZ_TRACE="1",
                       RUST_BACKTRACE="1")
            process = subprocess.Popen(
                [binary, TEST, "--nocapture"], cwd=ROOT, env=env,
                stdout=log, stderr=subprocess.STDOUT,
            )
            running[seed] = (process, log)
        time.sleep(0.05)
        for seed, (process, log) in list(running.items()):
            code = process.poll()
            if code is None:
                continue
            log.close()
            del running[seed]
            results[seed] = "ok" if code == 0 else f"failed ({code})"
            if results[seed] != "ok":
                print(f"seed {seed}: {results[seed]}", flush=True)

    failed = {s: r for s, r in results.items() if r != "ok"}
    reasons = Counter()
    for seed in failed:
        text = (logs / f"{seed}.log").read_text(errors="replace")
        match = re.search(r"panicked at [^\n]*\n([^\n]*)", text)
        reasons[match.group(1) if match else failed[seed]] += 1
    print(f"\n{len(results) - len(failed)}/{len(results)} seeds passed. Logs in {logs}")
    for reason, count in reasons.most_common():
        print(f"  {count} × {reason}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
