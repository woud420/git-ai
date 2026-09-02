#!/usr/bin/env python3
"""Compare scoped checkpoint latency without conflating acknowledgement contracts.

The candidate and baseline run as release binaries against isolated, matched Git
fixtures. Each pair is alternated AB/BA. The default profile records but does not
compare unlike command-return lanes; the fork-before-after profile compares the
shared live-application acknowledgement boundary.
"""

from __future__ import annotations

import argparse
import json
import os
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

GIT_BENCHMARK_DIR = Path(__file__).resolve().parents[1] / "git"
sys.path.insert(0, str(GIT_BENCHMARK_DIR))

from benchmark_common import resolve_real_git_binary  # noqa: E402

from scoped_checkpoint_contract import (
    COMPARISON_PROFILES,
    DEFAULT_COMPARISON_PROFILE,
    canonical_digest,
    file_set_digest,
    resolve_comparison_profile,
    sha256_file,
)
from scoped_checkpoint_runner import run_scenario


SCHEMA = "git-ai-scoped-checkpoint-comparison/1.3.0"
MIN_DECISION_SAMPLES = 20
QUALIFIED_SAMPLES = 30
QUALIFIED_WARMUPS = 5
QUALIFIED_RESOURCE_SAMPLES = 20
QUALIFIED_PREFILL_DEPTHS = {0, 50, 200}
QUALIFIED_BOOTSTRAP_RESAMPLES = 10_000
QUALIFIED_BOOTSTRAP_SEED = 364
DEFAULT_BASELINE_REF = "6fbc1ef0f4d40232315efc1b907e7ff5526dbea7"


def command_output(command: list[str], *, cwd: Path | None = None) -> str:
    return subprocess.run(
        command,
        cwd=cwd,
        text=True,
        capture_output=True,
        check=True,
    ).stdout.strip()


def source_metadata(
    source_dir: Path, *, allow_dirty: bool, git_binary: str
) -> dict[str, Any]:
    source_dir = source_dir.resolve()
    commit = command_output([git_binary, "rev-parse", "HEAD"], cwd=source_dir)
    status = command_output([git_binary, "status", "--porcelain"], cwd=source_dir)
    if status and not allow_dirty:
        raise RuntimeError(
            f"source worktree is dirty: {source_dir}; commit/stash it or pass "
            "--allow-dirty-sources for a non-decision smoke run"
        )
    return {
        "path": str(source_dir),
        "commit": commit,
        "dirty": bool(status),
        "dirty_status": status.splitlines(),
        "cargo_lock_sha256": sha256_file(source_dir / "Cargo.lock"),
    }


def assert_source_metadata_unchanged(
    name: str, expected: dict[str, Any], observed: dict[str, Any]
) -> None:
    keys = ("commit", "dirty", "dirty_status", "cargo_lock_sha256")
    mismatches = [key for key in keys if expected.get(key) != observed.get(key)]
    if mismatches:
        raise RuntimeError(
            f"{name} source changed during the run: {', '.join(mismatches)}"
        )


def binary_metadata(binary: Path) -> dict[str, Any]:
    binary = binary.resolve()
    return {
        "path": str(binary),
        "sha256": sha256_file(binary),
        "size_bytes": binary.stat().st_size,
        "version": command_output([str(binary), "--version"]),
    }


def build_binary(
    source_dir: Path,
    target_dir: Path,
    source: dict[str, Any],
) -> tuple[Path, dict[str, Any]]:
    cargo = shutil.which("cargo")
    rustc = shutil.which("rustc")
    if cargo is None or rustc is None:
        raise RuntimeError("cargo and rustc are required when binaries are not supplied")
    command = [cargo, "build", "--locked", "--offline", "--release", "--bin", "git-ai"]
    env = dict(os.environ)
    cleared = (
        "CARGO_BUILD_TARGET",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTFLAGS",
    )
    for key in cleared:
        env.pop(key, None)
    env.update(
        {
            "CARGO_NET_OFFLINE": "true",
            "CARGO_TARGET_DIR": str(target_dir),
            "LC_ALL": "C",
            "SOURCE_DATE_EPOCH": "946684800",
            "TZ": "UTC",
        }
    )
    started = time.perf_counter()
    result = subprocess.run(
        command,
        cwd=source_dir,
        env=env,
        text=True,
        capture_output=True,
    )
    duration_seconds = time.perf_counter() - started
    if result.returncode != 0:
        raise RuntimeError(
            f"release build failed in {source_dir}\nstdout:\n{result.stdout}\n"
            f"stderr:\n{result.stderr}"
        )
    binary = target_dir / "release" / "git-ai"
    if not binary.is_file():
        raise RuntimeError(f"release build did not produce {binary}")
    manifest = {
        "status": "verified_local_build",
        "source_commit": source["commit"],
        "cargo_lock_sha256": source["cargo_lock_sha256"],
        "command": command,
        "cleared_environment_keys": list(cleared),
        "fixed_environment": {
            "CARGO_NET_OFFLINE": "true",
            "LC_ALL": "C",
            "SOURCE_DATE_EPOCH": "946684800",
            "TZ": "UTC",
        },
        "cargo_path": cargo,
        "cargo_version": command_output([cargo, "-V"]),
        "rustc_path": rustc,
        "rustc_vv": command_output([rustc, "-Vv"]),
        "duration_seconds": duration_seconds,
        "stdout_sha256": canonical_digest(result.stdout),
        "stderr_sha256": canonical_digest(result.stderr),
        "binary_sha256": sha256_file(binary),
    }
    manifest["manifest_digest"] = canonical_digest(manifest)
    return binary, manifest


def environment_metadata(git_binary: str) -> dict[str, Any]:
    rustc = shutil.which("rustc")
    try:
        physical_memory = os.sysconf("SC_PAGE_SIZE") * os.sysconf("SC_PHYS_PAGES")
    except (AttributeError, OSError, ValueError):
        physical_memory = "unavailable"
    cpu_model = platform.processor() or platform.machine()
    if platform.system() == "Darwin":
        try:
            cpu_model = command_output(["sysctl", "-n", "machdep.cpu.brand_string"])
        except subprocess.CalledProcessError:
            pass
    return {
        "platform": platform.platform(),
        "machine": platform.machine(),
        "cpu_model": cpu_model,
        "logical_cpu_count": os.cpu_count(),
        "physical_memory_bytes": physical_memory,
        "load_average_at_start": list(os.getloadavg()) if hasattr(os, "getloadavg") else None,
        "power_state": "uncontrolled",
        "python": sys.version,
        "python_executable": sys.executable,
        "git_binary": git_binary,
        "git_binary_sha256": sha256_file(Path(git_binary)),
        "git_version": command_output([git_binary, "--version"]),
        "rustc_vv": command_output([rustc, "-Vv"]) if rustc else "unavailable",
        "clock": "time.perf_counter_ns",
        "resource_collector": (
            "/usr/bin/time -lp" if platform.system() == "Darwin" else "unavailable"
        ),
    }


def parse_depths(value: str) -> list[int]:
    depths = [int(part.strip()) for part in value.split(",") if part.strip()]
    if not depths or any(depth < 0 for depth in depths):
        raise argparse.ArgumentTypeError("prefill depths must be comma-separated non-negative ints")
    return depths


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--candidate-bin",
        type=Path,
        help="prebuilt smoke-only binary; omit both binary flags for verified local builds",
    )
    parser.add_argument("--baseline-bin", type=Path)
    parser.add_argument("--candidate-source", required=True, type=Path)
    parser.add_argument("--baseline-source", required=True, type=Path)
    parser.add_argument("--baseline-ref", default=DEFAULT_BASELINE_REF)
    parser.add_argument(
        "--comparison-profile",
        choices=sorted(COMPARISON_PROFILES),
        default=DEFAULT_COMPARISON_PROFILE,
        help=(
            "acknowledgement contract pairing; the default preserves the "
            "fork-vs-upstream comparison"
        ),
    )
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--warmups", type=int, default=5)
    parser.add_argument("--resource-samples", type=int, default=20)
    parser.add_argument("--prefill-depths", type=parse_depths, default=[0])
    parser.add_argument("--bootstrap-resamples", type=int, default=10_000)
    parser.add_argument("--bootstrap-seed", type=int, default=364)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--keep-fixtures", action="store_true")
    parser.add_argument("--debug-stages", action="store_true")
    parser.add_argument("--allow-small-sample", action="store_true")
    parser.add_argument("--allow-dirty-sources", action="store_true")
    args = parser.parse_args()
    if args.samples < MIN_DECISION_SAMPLES and not args.allow_small_sample:
        parser.error(
            f"--samples must be at least {MIN_DECISION_SAMPLES}; "
            "--allow-small-sample marks a smoke run as non-decision evidence"
        )
    if (
        args.samples <= 0
        or args.warmups < 0
        or args.resource_samples < 0
        or args.bootstrap_resamples <= 0
    ):
        parser.error("samples/resamples must be positive; warmups/resources non-negative")
    if platform.system() == "Windows":
        parser.error("sync.family probing currently requires Unix-domain sockets")
    if (args.candidate_bin is None) != (args.baseline_bin is None):
        parser.error("supply both --candidate-bin and --baseline-bin, or omit both")
    return args


def protocol_for_profile(comparison_profile: str) -> dict[str, Any]:
    profile = resolve_comparison_profile(comparison_profile)
    return {
        "comparison_profile": comparison_profile,
        "command_ack": profile["command_ack"],
        "command_ack_comparability": (
            "comparable_same_contract"
            if profile["command_ack_comparable"]
            else "not_comparable_different_contracts"
        ),
        "family_sync_fence": "process start through sync.family response",
        "material_observed": (
            "family sync followed by an exact unique index record and exact blob "
            "content observation; includes harness read/parse time"
        ),
        "candidate_ack_contract_id": profile["candidate_ack_contract_id"],
        "baseline_ack_contract_id": profile["baseline_ack_contract_id"],
        "candidate_ack_contract": profile["candidate_ack_contract"],
        "baseline_ack_contract": profile["baseline_ack_contract"],
        "pairing": "alternating AB/BA",
        "clock": "time.perf_counter_ns",
        "bootstrap": "paired index resampling with fixed seed",
        "checkpoint_storage": {
            "scope": (
                "logical bytes of working_logs/*/checkpoints.jsonl and "
                "working_logs/*/blobs/*"
            ),
            "measurement_phase": "after_daemon_shutdown",
            "timing_effect": "outside latency and process-resource samples",
        },
    }


def protocol_for_run(args: argparse.Namespace) -> dict[str, Any]:
    protocol = protocol_for_profile(args.comparison_profile)
    protocol["measurement"] = {
        "samples": args.samples,
        "warmups": args.warmups,
        "resource_samples": args.resource_samples,
        "prefill_depths": args.prefill_depths,
        "bootstrap_resamples": args.bootstrap_resamples,
        "bootstrap_seed": args.bootstrap_seed,
        "debug_stages": args.debug_stages,
        "per_depth_seed": "bootstrap_seed + prefill_depth",
    }
    return protocol


def verify_baseline_ref(
    source_dir: Path, expected_ref: str, actual_commit: str, git_binary: str
) -> str:
    resolved = command_output(
        [git_binary, "rev-parse", f"{expected_ref}^{{commit}}"], cwd=source_dir
    )
    if resolved != actual_commit:
        raise RuntimeError(
            f"baseline source is {actual_commit}, but --baseline-ref resolves to {resolved}"
        )
    return resolved


def harness_files(script_path: Path) -> list[Path]:
    return [
        script_path,
        script_path.with_name("scoped_checkpoint_contract.py"),
        script_path.with_name("scoped_checkpoint_runner.py"),
        GIT_BENCHMARK_DIR / "benchmark_common.py",
    ]


def has_qualified_measurement_protocol(args: argparse.Namespace) -> bool:
    return (
        args.samples == QUALIFIED_SAMPLES
        and args.warmups == QUALIFIED_WARMUPS
        and args.resource_samples == QUALIFIED_RESOURCE_SAMPLES
        and args.prefill_depths == sorted(QUALIFIED_PREFILL_DEPTHS)
        and args.bootstrap_resamples == QUALIFIED_BOOTSTRAP_RESAMPLES
        and args.bootstrap_seed == QUALIFIED_BOOTSTRAP_SEED
    )


def has_qualified_resource_evidence(
    scenarios: list[dict[str, Any]], expected_samples: int
) -> bool:
    client_keys = ("instructions_retired", "max_rss_bytes")
    daemon_keys = ("user_seconds", "sys_seconds", "max_rss_bytes")
    for scenario in scenarios:
        for variant in scenario.get("variants", {}).values():
            client = variant.get("client_resources", {})
            daemon = variant.get("daemon_resources", {})
            if any(client.get(key, {}).get("count") != expected_samples for key in client_keys):
                return False
            if any(not isinstance(daemon.get(key), (int, float)) for key in daemon_keys):
                return False
    return bool(scenarios)


def build_result(
    *,
    args: argparse.Namespace,
    script_path: Path,
    git_binary: str,
    source: dict[str, dict[str, Any]],
    binary: dict[str, dict[str, Any]],
    build_provenance: dict[str, dict[str, Any]],
    scenarios: list[dict[str, Any]],
    environment: dict[str, Any],
) -> dict[str, Any]:
    comparison_profile = getattr(
        args, "comparison_profile", DEFAULT_COMPARISON_PROFILE
    )
    protocol = protocol_for_run(args)
    fixture_contract = {
        "schema": "git-ai-scoped-checkpoint-fixture/1.0.0",
        "repo": "one commit on main with sample.txt and distractor.txt",
        "path": "sample.txt",
        "dirty_distractor": "distractor.txt must remain absent from every checkpoint",
        "seed": "seed\\n",
        "edits": "sha256(phase:index) hex plus newline",
        "checkpoint": ["checkpoint", "mock_ai", "sample.txt"],
        "material_fence": "sync.family then exact path/blob match in checkpoints.jsonl",
    }
    normalized_config = {
        "allowed_repositories": ["$VARIANT_ROOT/repo"],
        "git_path": git_binary,
    }
    run_contract_identity = {
        "harness_digest": file_set_digest(harness_files(script_path)),
        "fixture_digest": canonical_digest(fixture_contract),
        "config_digest": canonical_digest(normalized_config),
        "environment_digest": canonical_digest(environment),
        "protocol_digest": canonical_digest(protocol),
    }
    return {
        "schema": SCHEMA,
        "created_at": datetime.now(UTC).isoformat(),
        "qualification": (
            "decision_evidence"
            if has_qualified_measurement_protocol(args)
            and has_qualified_resource_evidence(scenarios, args.resource_samples)
            and not args.allow_dirty_sources
            and not args.debug_stages
            and all(
                value["status"] == "verified_local_build"
                for value in build_provenance.values()
            )
            and all(
                scenario["fixture_identity_check"]["status"] == "comparable"
                for scenario in scenarios
            )
            else "smoke_or_diagnostic_non_decision"
        ),
        "protocol": protocol,
        "parameters": {
            "samples": args.samples,
            "warmups": args.warmups,
            "resource_samples": args.resource_samples,
            "prefill_depths": args.prefill_depths,
            "bootstrap_resamples": args.bootstrap_resamples,
            "bootstrap_seed": args.bootstrap_seed,
            "debug_stages": args.debug_stages,
            "baseline_ref": args.baseline_ref,
            "comparison_profile": comparison_profile,
        },
        "run_contract_identity": run_contract_identity,
        "fixture_contract": fixture_contract,
        "normalized_config": normalized_config,
        "environment": environment,
        "variants": {
            "candidate": {
                "source": source["candidate"],
                "binary": binary["candidate"],
                "build": build_provenance["candidate"],
            },
            "baseline": {
                "source": source["baseline"],
                "binary": binary["baseline"],
                "build": build_provenance["baseline"],
            },
        },
        "scenarios": scenarios,
    }


def main() -> int:
    args = parse_args()
    script_path = Path(__file__).resolve()
    git_binary = str(resolve_real_git_binary(args.candidate_source))
    environment = environment_metadata(git_binary)
    for path in (args.candidate_bin, args.baseline_bin):
        if path is not None and not path.is_file():
            raise FileNotFoundError(path)

    source = {
        "candidate": source_metadata(
            args.candidate_source,
            allow_dirty=args.allow_dirty_sources,
            git_binary=git_binary,
        ),
        "baseline": source_metadata(
            args.baseline_source,
            allow_dirty=args.allow_dirty_sources,
            git_binary=git_binary,
        ),
    }
    verify_baseline_ref(
        args.baseline_source,
        args.baseline_ref,
        source["baseline"]["commit"],
        git_binary,
    )
    run_root = Path(tempfile.mkdtemp(prefix="git-ai-eng-364-"))
    try:
        if args.candidate_bin is None:
            candidate_path, candidate_build = build_binary(
                args.candidate_source,
                run_root / "build-candidate",
                source["candidate"],
            )
            baseline_path, baseline_build = build_binary(
                args.baseline_source,
                run_root / "build-baseline",
                source["baseline"],
            )
        else:
            candidate_path = args.candidate_bin
            baseline_path = args.baseline_bin
            candidate_build = {"status": "external_binary_provenance_unverified"}
            baseline_build = {"status": "external_binary_provenance_unverified"}
        assert baseline_path is not None
        binary = {
            "candidate": binary_metadata(candidate_path),
            "baseline": binary_metadata(baseline_path),
        }
        build_provenance = {
            "candidate": candidate_build,
            "baseline": baseline_build,
        }
        scenarios = [
            run_scenario(
                scenario_root=run_root / f"depth-{depth}",
                candidate_binary=candidate_path,
                baseline_binary=baseline_path,
                git_binary=git_binary,
                prefill_depth=depth,
                warmups=args.warmups,
                samples=args.samples,
                resource_samples=args.resource_samples,
                debug_stages=args.debug_stages,
                bootstrap_resamples=args.bootstrap_resamples,
                bootstrap_seed=args.bootstrap_seed + depth,
                comparison_profile=args.comparison_profile,
            )
            for depth in args.prefill_depths
        ]
        for name, source_dir in (
            ("candidate", args.candidate_source),
            ("baseline", args.baseline_source),
        ):
            observed = source_metadata(
                source_dir,
                allow_dirty=args.allow_dirty_sources,
                git_binary=git_binary,
            )
            assert_source_metadata_unchanged(name, source[name], observed)
            source[name]["verified_unchanged_after_run"] = True
        result = build_result(
            args=args,
            script_path=script_path,
            git_binary=git_binary,
            source=source,
            binary=binary,
            build_provenance=build_provenance,
            scenarios=scenarios,
            environment=environment,
        )
        serialized = json.dumps(result, sort_keys=True, indent=2) + "\n"
        if args.output:
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(serialized, encoding="utf-8")
            print(f"wrote {args.output.resolve()}")
        else:
            print(serialized, end="")

        for scenario in scenarios:
            comparison = scenario["comparisons"]["family_sync_fence_ms"]
            if comparison["comparison_status"] == "comparable":
                ratio = comparison["paired_ratio"]
                summary = (
                    f"candidate={comparison['candidate']['p50']:.3f} "
                    f"baseline={comparison['baseline']['p50']:.3f} "
                    f"ratio={ratio['candidate_over_baseline']:.3f} "
                    f"95% CI=[{ratio['bootstrap_95_ci'][0]:.3f}, "
                    f"{ratio['bootstrap_95_ci'][1]:.3f}]"
                )
            else:
                summary = f"not comparable: {comparison['reason']}"
            print(
                f"depth={scenario['prefill_depth']} family-sync p50 ms: {summary}",
                file=sys.stderr,
            )
        return 0
    finally:
        if args.keep_fixtures:
            print(f"kept fixtures at {run_root}", file=sys.stderr)
        else:
            shutil.rmtree(run_root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
