#!/usr/bin/env python3
"""Fail unless every test shard for one OS finished successfully.

The summary job starts once its `needs` are done, but the jobs API can still
report a shard that just finished as in progress for a few seconds. Reading it
once would fail a green run, so this re-reads until every shard has completed.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import time
from collections.abc import Callable

POLL_SECONDS = 5
MAX_POLLS = 24  # two minutes


def fetch_run_jobs() -> list[dict]:
    output = subprocess.run(
        ["gh", "run", "view", os.environ["GITHUB_RUN_ID"], "--json", "jobs"],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return json.loads(output)["jobs"]


def shard_jobs(jobs: list[dict], prefix: str) -> list[dict]:
    return sorted(
        (job for job in jobs if job["name"].startswith(prefix)),
        key=lambda job: job["name"],
    )


def settled(shards: list[dict], expected: int) -> bool:
    return len(shards) == expected and all(
        job["status"] == "completed" for job in shards
    )


def check_shards(
    prefix: str,
    expected: int,
    label: str,
    fetch: Callable[[], list[dict]] = fetch_run_jobs,
    sleep: Callable[[float], None] = time.sleep,
) -> int:
    for poll in range(MAX_POLLS):
        shards = shard_jobs(fetch(), prefix)
        if settled(shards, expected) or poll + 1 == MAX_POLLS:
            break
        sleep(POLL_SECONDS)

    if len(shards) != expected:
        print(
            f"Expected {expected} jobs with prefix {prefix!r}, found {len(shards)}",
            file=sys.stderr,
        )
        for job in shards:
            print(f"  {job['name']}: {job['status']} / {job['conclusion']}")
        return 1

    for job in shards:
        print(f"{job['name']}: {job['status']} / {job['conclusion']}")
    if any(job["conclusion"] != "success" for job in shards):
        print(f"One or more {label} test shards failed", file=sys.stderr)
        return 1
    print(f"{label} test shards completed successfully")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--prefix", required=True, help="shared job-name prefix")
    parser.add_argument("--expected", type=int, required=True, help="shard count")
    parser.add_argument("--label", required=True, help="OS name for messages")
    args = parser.parse_args()
    return check_shards(args.prefix, args.expected, args.label)


if __name__ == "__main__":
    raise SystemExit(main())
