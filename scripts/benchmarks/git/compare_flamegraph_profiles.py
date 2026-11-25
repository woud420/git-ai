#!/usr/bin/env python3
"""Compare two collapsed (folded) daemon profiles and report where time moved.

The profiles come from `perf record -e cpu-clock`, so each stack's weight is
on-CPU nanoseconds and a profile's total is the daemon's sampled CPU time.
Frames are compared by their share of each profile; the z-score says how far a
change is from sampling noise.
"""

from __future__ import annotations

import argparse
import math
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

# perf's placeholder for frames it could not symbolize; it lumps unrelated
# code together, so it would only add noise to the tables.
UNKNOWN_FRAME = "[unknown]"
# Below this many samples per profile, shifts of a few points are noise.
LOW_SAMPLE_COUNT = 1000
# |z| at or above this is unlikely to be sampling noise.
SIGNIFICANT_Z = 3.0
MAX_FRAME_CHARS = 120


@dataclass
class Profile:
    total_weight: int
    samples: int
    inclusive: Counter[str]  # weight of stacks that contain the frame
    self_time: Counter[str]  # weight of stacks whose top frame it is


@dataclass
class FrameDelta:
    frame: str
    baseline_share: float
    candidate_share: float
    z: float

    @property
    def delta(self) -> float:
        return self.candidate_share - self.baseline_share


def parse_folded(text: str) -> Profile:
    inclusive: Counter[str] = Counter()
    self_time: Counter[str] = Counter()
    total_weight = 0
    period = 0
    for line in text.splitlines():
        stack, _, count = line.rstrip().rpartition(" ")
        if not stack or not count.isdigit():
            continue
        weight = int(count)
        frames = [frame for frame in stack.split(";") if frame != UNKNOWN_FRAME]
        total_weight += weight
        # Every cpu-clock sample carries the same period, so the gcd of the
        # weights is that period as soon as one stack was sampled once, which a
        # real profile's long tail guarantees.
        period = math.gcd(period, weight)
        for frame in set(frames):
            inclusive[frame] += weight
        if frames:
            self_time[frames[-1]] += weight
    samples = total_weight // period if period else 0
    return Profile(total_weight, samples, inclusive, self_time)


def share_z(p1: float, n1: int, p2: float, n2: int) -> float:
    variance = p1 * (1 - p1) / n1 + p2 * (1 - p2) / n2
    if variance == 0:
        return 0.0
    return (p2 - p1) / math.sqrt(variance)


def frame_deltas(
    baseline: Profile, candidate: Profile, weights: str, limit: int
) -> list[FrameDelta]:
    if baseline.samples == 0 or candidate.samples == 0:
        return []
    base_weights: Counter[str] = getattr(baseline, weights)
    cand_weights: Counter[str] = getattr(candidate, weights)
    rows = []
    for frame in base_weights.keys() | cand_weights.keys():
        p1 = base_weights[frame] / baseline.total_weight
        p2 = cand_weights[frame] / candidate.total_weight
        rows.append(
            FrameDelta(
                frame, p1, p2, share_z(p1, baseline.samples, p2, candidate.samples)
            )
        )
    rows.sort(key=lambda row: (-abs(row.delta), row.frame))
    return rows[:limit]


def markdown_frame(frame: str) -> str:
    if len(frame) > MAX_FRAME_CHARS:
        frame = frame[: MAX_FRAME_CHARS - 1] + "…"
    return "`" + frame.replace("`", "'").replace("|", "\\|") + "`"


def frame_table(title: str, rows: list[FrameDelta]) -> list[str]:
    lines = [
        f"### {title}",
        "",
        "| Frame | main | current | Δ points | z |",
        "|---|---:|---:|---:|---:|",
    ]
    for row in rows:
        z = f"{row.z:+.1f}"
        if abs(row.z) >= SIGNIFICANT_Z:
            z = f"**{z}**"
        lines.append(
            f"| {markdown_frame(row.frame)} | {row.baseline_share:.1%} | "
            f"{row.candidate_share:.1%} | {row.delta * 100:+.2f} | {z} |"
        )
    if not rows:
        lines.append("| (no samples) | | | | |")
    lines.append("")
    return lines


def render_report(baseline: Profile, candidate: Profile, limit: int) -> str:
    def cpu_seconds(profile: Profile) -> str:
        return f"{profile.total_weight / 1e9:.3f}"

    cpu_change = (
        f"{candidate.total_weight / baseline.total_weight - 1:+.1%}"
        if baseline.total_weight
        else "n/a"
    )
    lines = [
        "## Daemon profile: current vs main",
        "",
        "| | main | current | Change |",
        "|---|---:|---:|---:|",
        (
            f"| Sampled on-CPU time (s) | {cpu_seconds(baseline)} | "
            f"{cpu_seconds(candidate)} | {cpu_change} |"
        ),
        f"| Samples | {baseline.samples:,} | {candidate.samples:,} | |",
        "",
        (
            "Shares are the percentage of each profile's samples. "
            f"**Bold** z-scores (|z| ≥ {SIGNIFICANT_Z:g}) are unlikely to be sampling noise."
        ),
        "",
    ]
    if min(baseline.samples, candidate.samples) < LOW_SAMPLE_COUNT:
        lines += [
            (
                f"> **Warning:** a profile has fewer than {LOW_SAMPLE_COUNT:,} samples, "
                "so only large changes are meaningful."
            ),
            "",
        ]
    lines += frame_table(
        "Largest changes in inclusive time (frame anywhere on the stack)",
        frame_deltas(baseline, candidate, "inclusive", limit),
    )
    lines += frame_table(
        "Largest changes in self time (frame on top of the stack)",
        frame_deltas(baseline, candidate, "self_time", limit),
    )
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("baseline", type=Path, help="main's folded profile")
    parser.add_argument("candidate", type=Path, help="current branch's folded profile")
    parser.add_argument("--limit", type=int, default=20, help="rows per table")
    parser.add_argument(
        "--output", type=Path, help="write the report here instead of stdout"
    )
    args = parser.parse_args()

    report = render_report(
        parse_folded(args.baseline.read_text(encoding="utf-8")),
        parse_folded(args.candidate.read_text(encoding="utf-8")),
        args.limit,
    )
    if args.output is None:
        print(report)
    else:
        args.output.write_text(report + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
