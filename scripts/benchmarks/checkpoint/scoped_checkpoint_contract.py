"""Pure statistics, evidence-contract, and log parsing for the benchmark."""

from __future__ import annotations

import hashlib
import json
import math
import random
import re
import statistics
from pathlib import Path
from typing import Any


COMMON_IDENTITY_KEYS = (
    "harness_digest",
    "fixture_digest",
    "config_digest",
    "environment_digest",
    "protocol_digest",
)
DEFAULT_COMPARISON_PROFILE = "fork-vs-upstream-v1"
COMPARISON_PROFILES: dict[str, dict[str, Any]] = {
    DEFAULT_COMPARISON_PROFILE: {
        "candidate_ack_contract_id": "fork-live-application/v1",
        "baseline_ack_contract_id": "upstream-bounded-receipt/v1",
        "candidate_ack_contract": (
            "successful live checkpoint application before CLI return; storage and "
            "crash durability are evaluated by separate correctness evidence"
        ),
        "baseline_ack_contract": "bounded in-memory receipt before processing",
        "command_ack_comparable": False,
        "command_ack": "process start to CLI return; recorded, not compared",
        "command_ack_incomparable_reason": (
            "different acknowledgement contracts: candidate waits for side-effect "
            "completion; baseline acknowledges bounded in-memory receipt"
        ),
    },
    "fork-before-after-v1": {
        "candidate_ack_contract_id": "fork-live-application/v1",
        "baseline_ack_contract_id": "fork-live-application/v1",
        "candidate_ack_contract": (
            "successful live checkpoint application before CLI return; crash durability "
            "is evaluated by separate correctness guards"
        ),
        "baseline_ack_contract": (
            "successful live checkpoint application before CLI return; crash durability "
            "is evaluated by separate correctness guards"
        ),
        "command_ack_comparable": True,
        "command_ack": (
            "process start to CLI return; compared under the shared "
            "fork-live-application/v1 boundary"
        ),
        "command_ack_comparability_basis": (
            "shared fork-live-application/v1 acknowledgement boundary"
        ),
    },
}
TIME_METRIC_LABELS = {
    "maximum resident set size": "max_rss_bytes",
    "instructions retired": "instructions_retired",
    "cycles elapsed": "cycles_elapsed",
    "peak memory footprint": "peak_memory_footprint_bytes",
}


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_digest(value: Any) -> str:
    return sha256_bytes(
        json.dumps(value, sort_keys=True, separators=(",", ":")).encode("utf-8")
    )


def resolve_comparison_profile(name: str) -> dict[str, Any]:
    try:
        return dict(COMPARISON_PROFILES[name])
    except KeyError as error:
        choices = ", ".join(sorted(COMPARISON_PROFILES))
        raise ValueError(
            f"unknown comparison profile {name!r}; choose from {choices}"
        ) from error


def file_set_digest(paths: list[Path]) -> str:
    return canonical_digest(
        {path.name: sha256_file(path) for path in sorted(paths, key=lambda item: item.name)}
    )


def nearest_rank_percentile(values: list[float], quantile: float) -> float:
    if not values:
        raise ValueError("percentile requires at least one value")
    if not 0.0 <= quantile <= 1.0:
        raise ValueError("quantile must be between zero and one")
    ordered = sorted(values)
    rank = max(1, math.ceil(quantile * len(ordered)))
    return ordered[rank - 1]


def summarize_values(values: list[float]) -> dict[str, Any]:
    if not values:
        return {"count": 0, "status": "unavailable"}
    return {
        "count": len(values),
        "p50": statistics.median(values),
        "p95": nearest_rank_percentile(values, 0.95),
        "min": min(values),
        "max": max(values),
        "values": values,
    }


def _validate_pairs(candidate: list[float], baseline: list[float]) -> None:
    if not candidate or len(candidate) != len(baseline):
        raise ValueError("candidate and baseline must contain equal non-empty pairs")
    if any(value <= 0 for value in baseline):
        raise ValueError("baseline values must be positive")


def paired_bootstrap_ratio(
    candidate: list[float],
    baseline: list[float],
    *,
    resamples: int,
    seed: int,
) -> tuple[float, float, float]:
    _validate_pairs(candidate, baseline)
    if resamples <= 0:
        raise ValueError("bootstrap resamples must be positive")
    paired_ratios = [
        left / right for left, right in zip(candidate, baseline, strict=True)
    ]
    point = statistics.median(paired_ratios)
    generator = random.Random(seed)
    ratios = [
        statistics.median(
            [paired_ratios[generator.randrange(len(paired_ratios))] for _ in paired_ratios]
        )
        for _ in range(resamples)
    ]
    return (
        point,
        nearest_rank_percentile(ratios, 0.025),
        nearest_rank_percentile(ratios, 0.975),
    )


def paired_bootstrap_difference(
    candidate: list[float],
    baseline: list[float],
    *,
    resamples: int,
    seed: int,
) -> tuple[float, float, float]:
    _validate_pairs(candidate, baseline)
    differences = [left - right for left, right in zip(candidate, baseline, strict=True)]
    point = statistics.median(differences)
    generator = random.Random(seed)
    medians = [
        statistics.median(
            [differences[generator.randrange(len(differences))] for _ in differences]
        )
        for _ in range(resamples)
    ]
    return (
        point,
        nearest_rank_percentile(medians, 0.025),
        nearest_rank_percentile(medians, 0.975),
    )


def comparison_status(
    candidate: dict[str, str],
    baseline: dict[str, str],
    required_keys: tuple[str, ...] = COMMON_IDENTITY_KEYS,
) -> dict[str, Any]:
    mismatches = [
        key
        for key in required_keys
        if key not in candidate
        or key not in baseline
        or candidate[key] != baseline[key]
    ]
    if mismatches:
        return {
            "status": "not_comparable",
            "reason": "identity mismatch; rebaseline required",
            "mismatches": mismatches,
        }
    return {"status": "comparable", "mismatches": []}


def summarize_cross_variant_lane(
    candidate: list[float],
    baseline: list[float],
    *,
    lane: str,
    bootstrap_resamples: int,
    bootstrap_seed: int,
    comparison_profile: str = DEFAULT_COMPARISON_PROFILE,
) -> dict[str, Any]:
    profile = resolve_comparison_profile(comparison_profile)
    result: dict[str, Any] = {
        "candidate": summarize_values(candidate),
        "baseline": summarize_values(baseline),
    }
    if lane == "command_ack" and not profile["command_ack_comparable"]:
        result.update(
            {
                "comparison_status": "not_comparable",
                "reason": profile["command_ack_incomparable_reason"],
            }
        )
        return result

    ratio = paired_bootstrap_ratio(
        candidate,
        baseline,
        resamples=bootstrap_resamples,
        seed=bootstrap_seed,
    )
    difference = paired_bootstrap_difference(
        candidate,
        baseline,
        resamples=bootstrap_resamples,
        seed=bootstrap_seed + 1,
    )
    result.update(
        {
            "comparison_status": "comparable",
            "paired_ratio": {
                "candidate_over_baseline": ratio[0],
                "bootstrap_95_ci": [ratio[1], ratio[2]],
            },
            "paired_difference_ms": {
                "median": difference[0],
                "bootstrap_95_ci": [difference[1], difference[2]],
            },
        }
    )
    if lane == "command_ack":
        result["comparability_basis"] = profile[
            "command_ack_comparability_basis"
        ]
    return result


def durability_comparisons(comparison_profile: str) -> dict[str, Any]:
    if comparison_profile == DEFAULT_COMPARISON_PROFILE:
        return {
            "native_checkpoint_index_durability": {
                "comparison_status": "not_comparable",
                "candidate": "not_assessed_by_latency_harness",
                "baseline": "not_assessed_by_latency_harness",
                "reason": (
                    "latency does not establish storage-mechanism durability; use each "
                    "snapshot's correctness evidence"
                ),
            },
            "full_checkpoint_crash_durability": {
                "comparison_status": "not_comparable",
                "candidate": "not_assessed_by_latency_harness",
                "baseline": "not_assessed_by_latency_harness",
                "reason": (
                    "the harness validates materialized index and blob content but does "
                    "not simulate a process or machine crash"
                ),
            },
        }

    resolve_comparison_profile(comparison_profile)
    return {
        "native_checkpoint_index_durability": {
            "comparison_status": "not_comparable",
            "candidate": "not_assessed_by_latency_harness",
            "baseline": "not_assessed_by_latency_harness",
            "reason": (
                "before/after latency does not establish storage-mechanism durability; "
                "use the candidate and baseline correctness evidence"
            ),
        },
        "full_checkpoint_crash_durability": {
            "comparison_status": "not_comparable",
            "candidate": "not_assessed_by_latency_harness",
            "baseline": "unverified",
            "reason": (
                "the latency harness does not simulate a crash; the ENG-364 baseline's "
                "referenced blob fsync remains unverified"
            ),
        },
    }


def pair_order(index: int) -> tuple[str, str]:
    return ("candidate", "baseline") if index % 2 == 0 else ("baseline", "candidate")


def find_materialized_checkpoint(
    lines: list[str], *, expected_blob_sha: str, expected_path: str
) -> dict[str, Any]:
    matches: list[dict[str, Any]] = []
    for line in lines:
        if not line.strip():
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise RuntimeError(f"malformed checkpoints.jsonl: {error}") from error
        entries = record.get("entries")
        if record.get("kind") != "AiAgent" or not isinstance(entries, list):
            continue
        if len(entries) == 1 and (
            isinstance(entries[0], dict)
            and entries[0].get("file") == expected_path
            and entries[0].get("blob_sha") == expected_blob_sha
        ):
            matches.append(record)
    if len(matches) != 1:
        raise RuntimeError(
            "expected exactly one materialized AI checkpoint for "
            f"{expected_path} at blob {expected_blob_sha}, found {len(matches)}"
        )
    return matches[0]


def validate_materialized_blob(
    working_logs: Path, *, expected_blob_sha: str, expected_content: bytes
) -> Path:
    matches = list(working_logs.glob(f"*/blobs/{expected_blob_sha}"))
    if len(matches) != 1:
        raise RuntimeError(
            f"expected exactly one blob {expected_blob_sha}, found {len(matches)}"
        )
    if matches[0].read_bytes() != expected_content:
        raise RuntimeError(f"materialized blob content mismatch for {expected_blob_sha}")
    return matches[0]


def parse_time_metrics(stderr: str) -> dict[str, float | int]:
    metrics: dict[str, float | int] = {}
    for line in stderr.splitlines():
        simple = re.fullmatch(r"(real|user|sys)\s+([0-9.]+)", line.strip())
        if simple:
            metrics[f"{simple.group(1)}_seconds"] = float(simple.group(2))
            continue
        detailed = re.fullmatch(r"\s*(\d+)\s+(.+?)\s*", line)
        if detailed and detailed.group(2) in TIME_METRIC_LABELS:
            metrics[TIME_METRIC_LABELS[detailed.group(2)]] = int(detailed.group(1))
    return metrics


def parse_cli_stage_metrics(stderr: str) -> dict[str, float]:
    pattern = re.compile(r"\[perf\] checkpoint: ([a-z_]+)=([0-9.]+)ms")
    return {
        f"{match.group(1)}_ms": float(match.group(2))
        for match in pattern.finditer(stderr)
    }


def _duration_to_ms(value: str) -> float:
    match = re.fullmatch(r"([0-9.]+)(ns|µs|us|ms|s)", value)
    if not match:
        raise ValueError(f"unrecognized Rust duration: {value}")
    number = float(match.group(1))
    return number * {"ns": 1e-6, "µs": 1e-3, "us": 1e-3, "ms": 1.0, "s": 1e3}[
        match.group(2)
    ]


def parse_daemon_stage_metrics(log_text: str, sample_count: int) -> dict[str, Any]:
    patterns = {
        "checkpoint_total_ms": re.compile(r"checkpoint done .* duration_ms=(\d+)"),
        "checkpoint_run_ms": re.compile(
            r"\[BENCHMARK\] Total checkpoint run took ([0-9.]+(?:ns|µs|us|ms|s))"
        ),
        "working_log_append_ms": re.compile(
            r"\[BENCHMARK\] Appending checkpoint to working log took "
            r"([0-9.]+(?:ns|µs|us|ms|s))"
        ),
        "entry_computation_ms": re.compile(
            r"\[BENCHMARK\] get_checkpoint_entries generated .* took "
            r"([0-9.]+(?:ns|µs|us|ms|s))"
        ),
    }
    parsed: dict[str, Any] = {}
    match_counts: dict[str, int] = {}
    for name, pattern in patterns.items():
        values = [
            float(match.group(1))
            if name == "checkpoint_total_ms"
            else _duration_to_ms(match.group(1))
            for match in pattern.finditer(log_text)
        ]
        match_counts[name] = len(values)
        if len(values) >= sample_count:
            parsed[name] = summarize_values(values[-sample_count:])
        else:
            parsed[name] = {
                "status": "unavailable",
                "reason": f"expected {sample_count} ordered samples, found {len(values)}",
            }
    complete = all(count >= sample_count for count in match_counts.values())
    parsed["collection"] = {
        "status": "complete" if complete else "incomplete",
        "expected_measured_samples": sample_count,
        "raw_match_counts_including_prefill_and_warmup": match_counts,
        "correlation": "last N records from an isolated sequential checkpoint stream",
    }
    if complete:
        totals = parsed["checkpoint_total_ms"]["values"]
        runs = parsed["checkpoint_run_ms"]["values"]
        parsed["coarse_non_checkpoint_run_residual_ms"] = {
            **summarize_values(
                [max(0.0, total - run) for total, run in zip(totals, runs, strict=True)]
            ),
            "caveat": "checkpoint_total_ms is integer-quantized and excludes later response work",
        }
    return parsed
