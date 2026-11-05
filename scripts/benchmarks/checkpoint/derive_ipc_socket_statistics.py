#!/usr/bin/env python3
"""Reproduce exploratory socket intervals from a retained IPC measurement report."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from scoped_checkpoint_contract import (
    paired_bootstrap_difference,
    paired_bootstrap_ratio,
)


def derive(report: dict) -> list[dict]:
    derived = []
    for fixture in report["socket"]["results"]:
        capnp_ns = fixture["capnp"]["ack"]["ns_per_operation"]
        json_ns = fixture["json"]["ack"]["ns_per_operation"]
        # Each helper creates its own Random(seed), resetting the stream for every
        # statistic and fixture. There are no fixture offsets or shared RNG state.
        ratio = paired_bootstrap_ratio(capnp_ns, json_ns, resamples=10_000, seed=364)
        difference = paired_bootstrap_difference(
            [value / 1_000_000 for value in capnp_ns],
            [value / 1_000_000 for value in json_ns],
            resamples=10_000,
            seed=364,
        )
        derived.append(
            {
                "fixture": fixture["fixture"],
                "paired_median_capnp_over_json": ratio[0],
                "paired_ratio_bootstrap_95_ci": list(ratio[1:]),
                "paired_median_difference_ms": difference[0],
                "paired_difference_bootstrap_95_ci_ms": list(difference[1:]),
            }
        )
    return derived


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path, help="sanitized codec/socket JSON artifact")
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    derived = derive(report)
    published = {
        result["fixture"]: result
        for result in report["descriptive_statistics"]["socket"]
    }
    for result in derived:
        for key, value in result.items():
            if key == "fixture":
                continue
            expected = published[result["fixture"]][key]
            actual_values = value if isinstance(value, list) else [value]
            expected_values = expected if isinstance(expected, list) else [expected]
            if not all(
                math.isclose(actual, retained, rel_tol=1e-12, abs_tol=1e-12)
                for actual, retained in zip(actual_values, expected_values, strict=True)
            ):
                raise ValueError(f"published statistic differs: {result['fixture']} {key}")
    print(json.dumps(derived, indent=2))


if __name__ == "__main__":
    main()
