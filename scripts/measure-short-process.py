#!/usr/bin/env python3
"""Measure a short Pebble command with only the Python standard library."""

import json
import os
import resource
import statistics
import subprocess
import sys
import time


EXPECTED_STDOUT = b"pebble 1.0.0\n"
SAMPLES = 101


def binary_size(path):
    if sys.platform == "darwin":
        command = ["stat", "-f", "%z", path]
    else:
        command = ["stat", "-c", "%s", path]
    size = int(subprocess.check_output(command, text=True).strip())
    if size <= 0:
        raise ValueError("binary size must be positive")
    return size


def peak_rss_kib():
    rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
    if sys.platform == "darwin":
        rss = (rss + 1023) // 1024
    if rss <= 0:
        raise ValueError("peak RSS must be positive")
    return rss


def measure(command):
    durations = []
    for _sample in range(SAMPLES):
        started = time.perf_counter_ns()
        completed = subprocess.run(command, capture_output=True, check=False)
        elapsed = time.perf_counter_ns() - started
        if completed.returncode != 0:
            raise ValueError(f"command exited {completed.returncode}")
        if completed.stdout != EXPECTED_STDOUT:
            raise ValueError(f"unexpected stdout: {completed.stdout!r}")
        if completed.stderr:
            raise ValueError(f"unexpected stderr: {completed.stderr!r}")
        durations.append(elapsed)

    startup_ns = statistics.median(durations[1:])
    if startup_ns <= 0:
        raise ValueError("startup median must be positive")
    return startup_ns


def main():
    command = sys.argv[1:]
    if not command:
        raise ValueError("usage: measure-short-process.py <command> [arguments...]")
    if not os.path.isfile(command[0]):
        raise ValueError(f"command is not a regular file: {command[0]}")

    startup_ns = measure(command)
    print(
        json.dumps(
            {
                "schema": 1,
                "samples": SAMPLES,
                "binary_bytes": binary_size(command[0]),
                "startup_ns": startup_ns,
                "peak_rss_kib": peak_rss_kib(),
            },
            separators=(",", ":"),
        )
    )


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"short-process measurement failed: {error}", file=sys.stderr)
        raise SystemExit(1) from error
