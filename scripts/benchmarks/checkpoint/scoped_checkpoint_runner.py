"""Isolated daemon/fixture runner for scoped checkpoint comparisons."""

from __future__ import annotations

import hashlib
import json
import os
import platform
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

from scoped_checkpoint_contract import (
    DEFAULT_COMPARISON_PROFILE,
    FIXTURE_ALLOWED_REMOTE,
    FIXTURE_ALLOWLIST_CONFIG_KEY,
    FIXTURE_DENIED_REMOTE,
    canonical_digest,
    comparison_status,
    durability_comparisons,
    pair_order,
    parse_daemon_stage_metrics,
    parse_cli_stage_metrics,
    parse_time_metrics,
    sha256_bytes,
    summarize_cross_variant_lane,
    summarize_values,
)
from scoped_checkpoint_record import (
    find_materialized_checkpoint,
    validate_materialized_blob,
)


FIXTURE_IDENTITY_KEYS = (
    "head_commit",
    "tree",
    "worktree_status_digest",
    "distractor_content_sha256",
    "repo_config_digest",
    "git_ai_config_digest",
    "git_ai_config_readback_digest",
    "allowlist_denial_control_digest",
    "runner_environment_digest",
)


def run_checked(
    command: list[str], *, cwd: Path, env: dict[str, str]
) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(command, cwd=cwd, env=env, text=True, capture_output=True)
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
    return result


def send_control(socket_path: Path, payload: dict[str, Any]) -> dict[str, Any]:
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
        client.settimeout(15)
        client.connect(str(socket_path))
        client.sendall(json.dumps(payload, separators=(",", ":")).encode() + b"\n")
        response_bytes = bytearray()
        while not response_bytes.endswith(b"\n"):
            chunk = client.recv(65536)
            if not chunk:
                break
            response_bytes.extend(chunk)
    response = json.loads(response_bytes.decode("utf-8"))
    if not response.get("ok"):
        raise RuntimeError(f"daemon control request failed: {response}")
    return response


def isolated_environment(root: Path) -> dict[str, str]:
    return {
        "PATH": os.environ.get("PATH", os.defpath),
        "HOME": str(root / "home"),
        "TMPDIR": str(root / "tmp"),
        "LANG": "C",
        "LC_ALL": "C",
        "TZ": "UTC",
        "GIT_CONFIG_GLOBAL": str(root / "home" / ".gitconfig"),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_AUTHOR_DATE": "2000-01-01T00:00:00Z",
        "GIT_COMMITTER_DATE": "2000-01-01T00:00:00Z",
    }


def fixed_content(phase: str, index: int) -> bytes:
    return hashlib.sha256(f"{phase}:{index}".encode("utf-8")).hexdigest().encode() + b"\n"


def checkpoint_storage_state(working_logs: Path) -> dict[str, int]:
    journals = [
        path
        for path in sorted(working_logs.glob("*/checkpoints.jsonl"))
        if path.is_file()
    ]
    blobs = [
        path
        for path in sorted(working_logs.glob("*/blobs/*"))
        if path.is_file()
    ]
    journal_bytes = sum(path.stat().st_size for path in journals)
    blob_bytes = sum(path.stat().st_size for path in blobs)
    return {
        "checkpoint_journal_bytes": journal_bytes,
        "blob_count": len(blobs),
        "blob_bytes": blob_bytes,
        "total_checkpoint_bytes": journal_bytes + blob_bytes,
    }


def fixture_git_ai_config(git_binary: str) -> dict[str, Any]:
    return {
        FIXTURE_ALLOWLIST_CONFIG_KEY: [FIXTURE_ALLOWED_REMOTE],
        "git_path": str(Path(git_binary).resolve()),
    }


def verify_fixture_git_ai_config(
    *, binary: Path, git_binary: str, cwd: Path, env: dict[str, str]
) -> dict[str, Any]:
    expected = fixture_git_ai_config(git_binary)
    observed: dict[str, Any] = {}
    for key in (FIXTURE_ALLOWLIST_CONFIG_KEY, "git_path"):
        result = run_checked(
            [str(binary), "config", key],
            cwd=cwd,
            env=env,
        )
        try:
            observed[key] = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise RuntimeError(
                f"native config readback returned invalid JSON for {key}"
            ) from error
        if observed[key] != expected[key]:
            raise RuntimeError(
                f"effective {key} mismatch: expected {expected[key]!r}, "
                f"observed {observed[key]!r}"
            )
    return {
        "status": "verified",
        "method": "native-config-readback/v1",
        "allowlist_config_key": FIXTURE_ALLOWLIST_CONFIG_KEY,
        **observed,
    }


def create_short_socket_root() -> Path:
    conventional_tmp = Path("/tmp")
    parent = conventional_tmp if conventional_tmp.is_dir() else Path(tempfile.gettempdir())
    return Path(tempfile.mkdtemp(prefix="g364-sockets-", dir=parent))


class VariantRunner:
    def __init__(
        self,
        *,
        label: str,
        binary: Path,
        root: Path,
        socket_root: Path,
        git_binary: str,
        debug_stages: bool,
    ) -> None:
        self.label = label
        self.binary = binary.resolve()
        self.root = root
        self.home = root / "home"
        self.repo = root / "repo"
        self.socket_root = socket_root
        self.control_socket = socket_root / "control.sock"
        self.trace_socket = socket_root / "trace.sock"
        self.denied_repo = root / "denied-repo"
        self.daemon_stderr_path = root / "daemon-resource.log"
        self.git_binary = git_binary
        self.debug_stages = debug_stages
        self.daemon: subprocess.Popen[str] | None = None
        self.daemon_stderr_file: Any = None
        self.daemon_resources: dict[str, float | int] = {}
        self.git_ai_config_readback: dict[str, Any] = {}
        self.allowlist_denial_control: dict[str, Any] = {}

        env = isolated_environment(root)
        env.update(
            {
                "HOME": str(self.home),
                "GIT_CONFIG_GLOBAL": str(self.home / ".gitconfig"),
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_TERMINAL_PROMPT": "0",
                "GIT_AI_DEBUG": "1" if debug_stages else "0",
                "GIT_AI_DEBUG_PERFORMANCE": "1" if debug_stages else "0",
                "GIT_AI_DAEMON_HOME": str(self.home),
                "GIT_AI_DAEMON_CONTROL_SOCKET": str(self.control_socket),
                "GIT_AI_DAEMON_TRACE_SOCKET": str(self.trace_socket),
                "GIT_AI_DAEMON_CHECKPOINT_DELEGATE": "true",
            }
        )
        self.env = env

    def setup(self) -> None:
        self.home.mkdir(parents=True)
        Path(self.env["TMPDIR"]).mkdir(parents=True)
        self.repo.mkdir(parents=True)
        self.socket_root.mkdir(parents=True)
        config_dir = self.home / ".git-ai"
        config_dir.mkdir(parents=True)
        config = fixture_git_ai_config(self.git_binary)
        (config_dir / "config.json").write_text(
            json.dumps(config, sort_keys=True, indent=2) + "\n", encoding="utf-8"
        )
        self.git_ai_config_readback = verify_fixture_git_ai_config(
            binary=self.binary,
            git_binary=self.git_binary,
            cwd=self.root,
            env=self.env,
        )

        run_checked(
            [self.git_binary, "init", "-q", "-b", "main"],
            cwd=self.repo,
            env=self.env,
        )
        run_checked(
            [self.git_binary, "config", "user.name", "ENG-364 Benchmark"],
            cwd=self.repo,
            env=self.env,
        )
        run_checked(
            [self.git_binary, "config", "user.email", "eng364@example.invalid"],
            cwd=self.repo,
            env=self.env,
        )
        run_checked(
            [self.git_binary, "remote", "add", "origin", FIXTURE_ALLOWED_REMOTE],
            cwd=self.repo,
            env=self.env,
        )
        (self.repo / "sample.txt").write_bytes(b"seed\n")
        (self.repo / "distractor.txt").write_bytes(b"seed\n")
        run_checked(
            [self.git_binary, "add", "sample.txt", "distractor.txt"],
            cwd=self.repo,
            env=self.env,
        )
        run_checked(
            [self.git_binary, "commit", "-q", "-m", "seed"],
            cwd=self.repo,
            env=self.env,
        )
        (self.repo / "distractor.txt").write_bytes(b"must-remain-out-of-scope\n")

        self.denied_repo.mkdir(parents=True)
        for command in (
            [self.git_binary, "init", "-q", "-b", "main"],
            [self.git_binary, "config", "user.name", "ENG-364 Benchmark"],
            [self.git_binary, "config", "user.email", "eng364@example.invalid"],
            [self.git_binary, "remote", "add", "origin", FIXTURE_DENIED_REMOTE],
        ):
            run_checked(command, cwd=self.denied_repo, env=self.env)
        (self.denied_repo / "sample.txt").write_bytes(b"seed\n")
        run_checked(
            [self.git_binary, "add", "sample.txt"],
            cwd=self.denied_repo,
            env=self.env,
        )
        run_checked(
            [self.git_binary, "commit", "-q", "-m", "seed"],
            cwd=self.denied_repo,
            env=self.env,
        )

        self._configure_daemon_boundary(
            self.socket_root / "allowlist-control",
            self.root / "allowlist-control-daemon-home",
        )
        try:
            self._start_daemon()
            self.allowlist_denial_control = self.verify_allowlist_denial()
        finally:
            self.stop()
            self.daemon_resources = {}
            self._configure_daemon_boundary(self.socket_root, self.home)
        self._start_daemon()

    def fixture_identity(self) -> dict[str, Any]:
        roots = {
            str(self.root): "$VARIANT_ROOT",
            str(self.root.resolve()): "$VARIANT_ROOT",
            str(self.socket_root): "$SOCKET_ROOT",
            str(self.socket_root.resolve()): "$SOCKET_ROOT",
        }

        def normalized(value: str) -> str:
            for root, replacement in sorted(
                roots.items(), key=lambda item: len(item[0]), reverse=True
            ):
                value = value.replace(root, replacement)
            return value

        head = run_checked(
            [self.git_binary, "rev-parse", "HEAD"], cwd=self.repo, env=self.env
        ).stdout.strip()
        tree = run_checked(
            [self.git_binary, "rev-parse", "HEAD^{tree}"], cwd=self.repo, env=self.env
        ).stdout.strip()
        worktree_status = normalized(
            run_checked(
                [self.git_binary, "status", "--porcelain=v1"],
                cwd=self.repo,
                env=self.env,
            ).stdout
        )
        repo_config = normalized(
            (self.repo / ".git" / "config").read_text(encoding="utf-8")
        )
        git_ai_config = normalized(
            (self.home / ".git-ai" / "config.json").read_text(encoding="utf-8")
        )
        config_readback = {
            key: normalized(value) if isinstance(value, str) else value
            for key, value in self.git_ai_config_readback.items()
        }
        normalized_env = {key: normalized(value) for key, value in sorted(self.env.items())}
        return {
            "head_commit": head,
            "tree": tree,
            "worktree_status_digest": canonical_digest(worktree_status),
            "distractor_content_sha256": sha256_bytes(
                (self.repo / "distractor.txt").read_bytes()
            ),
            "repo_config_digest": canonical_digest(repo_config),
            "git_ai_config_digest": canonical_digest(git_ai_config),
            "git_ai_config_readback": config_readback,
            "git_ai_config_readback_digest": canonical_digest(config_readback),
            "allowlist_denial_control": self.allowlist_denial_control,
            "allowlist_denial_control_digest": canonical_digest(
                self.allowlist_denial_control
            ),
            "runner_environment_digest": canonical_digest(normalized_env),
        }

    def _configure_daemon_boundary(self, socket_root: Path, daemon_home: Path) -> None:
        socket_root.mkdir(parents=True, exist_ok=True)
        self.control_socket = socket_root / "control.sock"
        self.trace_socket = socket_root / "trace.sock"
        self.env["GIT_AI_DAEMON_HOME"] = str(daemon_home)
        self.env["GIT_AI_DAEMON_CONTROL_SOCKET"] = str(self.control_socket)
        self.env["GIT_AI_DAEMON_TRACE_SOCKET"] = str(self.trace_socket)

    def _start_daemon(self) -> None:
        command = [str(self.binary), "daemon", "run"]
        if platform.system() == "Darwin" and Path("/usr/bin/time").exists():
            command = ["/usr/bin/time", "-lp", *command]
        self.daemon_stderr_file = self.daemon_stderr_path.open("w", encoding="utf-8")
        self.daemon = subprocess.Popen(
            command,
            cwd=self.root,
            env=self.env,
            stdout=subprocess.DEVNULL,
            stderr=self.daemon_stderr_file,
            text=True,
        )
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            if self.control_socket.exists() and self.trace_socket.exists():
                return
            if self.daemon.poll() is not None:
                raise RuntimeError(
                    f"{self.label} daemon exited before readiness; see "
                    f"{self.daemon_stderr_path}"
                )
            time.sleep(0.01)
        raise RuntimeError(f"{self.label} daemon did not become ready within 15 seconds")

    def sync_family(self, repo: Path | None = None) -> None:
        send_control(
            self.control_socket,
            {
                "method": "sync.family",
                "params": {"repo_working_dir": str((repo or self.repo).resolve())},
            },
        )

    def verify_allowlist_denial(self) -> dict[str, Any]:
        (self.denied_repo / "sample.txt").write_bytes(
            fixed_content("allowlist-denial", 0)
        )
        run_checked(
            [str(self.binary), "checkpoint", "mock_ai", "sample.txt"],
            cwd=self.denied_repo,
            env=self.env,
        )
        self.sync_family(self.denied_repo)
        storage = checkpoint_storage_state(
            self.denied_repo / ".git" / "ai" / "working_logs"
        )
        if any(storage.values()):
            raise RuntimeError(
                "allowlist denial control materialized checkpoint storage for the "
                "mismatched-origin repository"
            )
        return {
            "status": "verified_denied",
            "method": "mismatched-origin-checkpoint/v1",
            "allowed_remote": FIXTURE_ALLOWED_REMOTE,
            "denied_remote": FIXTURE_DENIED_REMOTE,
            "fence": "sync.family",
            "checkpoint_storage": storage,
        }

    def _checkpoint_lines(self) -> list[str]:
        working_logs = self.repo / ".git" / "ai" / "working_logs"
        return [
            line
            for path in sorted(working_logs.glob("*/checkpoints.jsonl"))
            for line in path.read_text(encoding="utf-8").splitlines()
            if line.strip()
        ]

    def run_checkpoint(self, content: bytes, *, collect_resources: bool) -> dict[str, Any]:
        checkpoint_lines_before = self._checkpoint_lines()
        (self.repo / "sample.txt").write_bytes(content)
        expected_blob_sha = sha256_bytes(content)
        command = [str(self.binary), "checkpoint", "mock_ai", "sample.txt"]
        if (
            collect_resources
            and platform.system() == "Darwin"
            and Path("/usr/bin/time").exists()
        ):
            command = ["/usr/bin/time", "-lp", *command]

        started = time.perf_counter_ns()
        result = run_checked(command, cwd=self.repo, env=self.env)
        acknowledged = time.perf_counter_ns()
        self.sync_family()
        family_synced = time.perf_counter_ns()
        checkpoint_lines_after = self._checkpoint_lines()
        if len(checkpoint_lines_after) != len(checkpoint_lines_before) + 1:
            raise RuntimeError(
                "checkpoint command must append exactly one index record; "
                f"before={len(checkpoint_lines_before)}, "
                f"after={len(checkpoint_lines_after)}"
            )
        record = find_materialized_checkpoint(
            checkpoint_lines_after[len(checkpoint_lines_before) :],
            expected_blob_sha=expected_blob_sha,
            expected_path="sample.txt",
        )
        validate_materialized_blob(
            self.repo / ".git" / "ai" / "working_logs",
            expected_blob_sha=expected_blob_sha,
            expected_content=content,
        )
        observed = time.perf_counter_ns()
        return {
            "ack_ms": (acknowledged - started) / 1_000_000,
            "family_sync_fence_ms": (family_synced - started) / 1_000_000,
            "material_observed_ms": (observed - started) / 1_000_000,
            "family_sync_minus_ack_ms": (family_synced - acknowledged) / 1_000_000,
            "trace_id": record.get("trace_id"),
            "delivery_id": record.get("delivery_id"),
            "client_resources": parse_time_metrics(result.stderr)
            if collect_resources
            else {},
            "client_stages": parse_cli_stage_metrics(result.stderr)
            if self.debug_stages
            else {},
        }

    def stop(self) -> None:
        if self.daemon is None:
            return
        try:
            send_control(self.control_socket, {"method": "shutdown"})
        except (OSError, RuntimeError, json.JSONDecodeError):
            pass
        if self.daemon.poll() is None:
            try:
                self.daemon.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.daemon.terminate()
                try:
                    self.daemon.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.daemon.kill()
                    self.daemon.wait(timeout=5)
        self.daemon_stderr_file.close()
        self.daemon_resources = parse_time_metrics(
            self.daemon_stderr_path.read_text(encoding="utf-8")
        )
        self.daemon = None

    def internal_log_text(self) -> str:
        log_dir = self.home / ".git-ai" / "internal" / "daemon" / "logs"
        if not log_dir.exists():
            return ""
        return "\n".join(
            path.read_text(encoding="utf-8", errors="replace")
            for path in sorted(log_dir.glob("*.log"))
        )


def summarize_variant_samples(
    samples: list[dict[str, Any]], resource_samples: list[dict[str, Any]]
) -> dict[str, Any]:
    output = {
        "command_ack_ms": summarize_values([sample["ack_ms"] for sample in samples]),
        "family_sync_fence_ms": summarize_values(
            [sample["family_sync_fence_ms"] for sample in samples]
        ),
        "material_observed_ms": summarize_values(
            [sample["material_observed_ms"] for sample in samples]
        ),
        "family_sync_minus_ack_ms": summarize_values(
            [sample["family_sync_minus_ack_ms"] for sample in samples]
        ),
        "samples": samples,
        "resource_samples": resource_samples,
    }
    resource_names = sorted(
        {name for sample in resource_samples for name in sample["client_resources"]}
    )
    output["client_resources"] = {
        name: summarize_values(
            [
                float(sample["client_resources"][name])
                for sample in resource_samples
                if name in sample["client_resources"]
            ]
        )
        for name in resource_names
    }
    stage_names = sorted({name for sample in samples for name in sample["client_stages"]})
    output["client_stages"] = {
        name: summarize_values(
            [
                sample["client_stages"][name]
                for sample in samples
                if name in sample["client_stages"]
            ]
        )
        for name in stage_names
    }
    return output


def summarize_scenario_lane(
    candidate: list[float],
    baseline: list[float],
    *,
    lane: str,
    fixture_identity_check: dict[str, Any],
    bootstrap_resamples: int,
    bootstrap_seed: int,
    comparison_profile: str = DEFAULT_COMPARISON_PROFILE,
) -> dict[str, Any]:
    if fixture_identity_check["status"] != "comparable":
        return {
            "candidate": summarize_values(candidate),
            "baseline": summarize_values(baseline),
            "comparison_status": "not_comparable",
            "reason": "fixture identity mismatch; rebaseline required",
            "mismatches": fixture_identity_check["mismatches"],
        }
    return summarize_cross_variant_lane(
        candidate,
        baseline,
        lane=lane,
        bootstrap_resamples=bootstrap_resamples,
        bootstrap_seed=bootstrap_seed,
        comparison_profile=comparison_profile,
    )


def run_scenario(
    *,
    scenario_root: Path,
    candidate_binary: Path,
    baseline_binary: Path,
    git_binary: str,
    prefill_depth: int,
    warmups: int,
    samples: int,
    resource_samples: int,
    debug_stages: bool,
    bootstrap_resamples: int,
    bootstrap_seed: int,
    comparison_profile: str = DEFAULT_COMPARISON_PROFILE,
) -> dict[str, Any]:
    socket_root = create_short_socket_root()
    runners = {
        "candidate": VariantRunner(
            label="candidate",
            binary=candidate_binary,
            root=scenario_root / "candidate",
            socket_root=socket_root / "candidate",
            git_binary=git_binary,
            debug_stages=debug_stages,
        ),
        "baseline": VariantRunner(
            label="baseline",
            binary=baseline_binary,
            root=scenario_root / "baseline",
            socket_root=socket_root / "baseline",
            git_binary=git_binary,
            debug_stages=debug_stages,
        ),
    }
    collected: dict[str, list[dict[str, Any]]] = {"candidate": [], "baseline": []}
    resources: dict[str, list[dict[str, Any]]] = {"candidate": [], "baseline": []}
    stage_snapshots: dict[str, dict[str, Any]] = {}
    fixture_identities: dict[str, dict[str, str]] = {}
    try:
        for runner in runners.values():
            runner.setup()
        fixture_identities = {
            label: runner.fixture_identity() for label, runner in runners.items()
        }
        fixture_identity_check = comparison_status(
            fixture_identities["candidate"],
            fixture_identities["baseline"],
            FIXTURE_IDENTITY_KEYS,
        )
        for index in range(prefill_depth):
            content = fixed_content("prefill", index)
            for label in pair_order(index):
                runners[label].run_checkpoint(content, collect_resources=False)
        for index in range(warmups):
            content = fixed_content("warmup", index)
            for label in pair_order(prefill_depth + index):
                runners[label].run_checkpoint(content, collect_resources=False)
        orders: list[list[str]] = []
        for index in range(samples):
            content = fixed_content("sample", index)
            order = pair_order(index)
            orders.append(list(order))
            for label in order:
                sample = runners[label].run_checkpoint(content, collect_resources=False)
                sample["pair_index"] = index
                collected[label].append(sample)
        stage_snapshots = {
            label: parse_daemon_stage_metrics(runner.internal_log_text(), samples)
            for label, runner in runners.items()
        }
        for index in range(resource_samples):
            content = fixed_content("resource", index)
            for label in pair_order(index):
                sample = runners[label].run_checkpoint(content, collect_resources=True)
                sample["resource_index"] = index
                resources[label].append(sample)
    finally:
        for runner in runners.values():
            runner.stop()
        shutil.rmtree(socket_root, ignore_errors=True)

    variants: dict[str, Any] = {}
    for label, runner in runners.items():
        variants[label] = summarize_variant_samples(collected[label], resources[label])
        variants[label]["daemon_resources"] = runner.daemon_resources
        variants[label]["daemon_stages"] = stage_snapshots.get(label, {})
        variants[label]["checkpoint_storage"] = checkpoint_storage_state(
            runner.repo / ".git" / "ai" / "working_logs"
        )

    candidate_ack = [sample["ack_ms"] for sample in collected["candidate"]]
    baseline_ack = [sample["ack_ms"] for sample in collected["baseline"]]
    candidate_material = [
        sample["family_sync_fence_ms"] for sample in collected["candidate"]
    ]
    baseline_material = [
        sample["family_sync_fence_ms"] for sample in collected["baseline"]
    ]
    material_comparison = summarize_scenario_lane(
        candidate_material,
        baseline_material,
        lane="family_sync_fence",
        fixture_identity_check=fixture_identity_check,
        bootstrap_resamples=bootstrap_resamples,
        bootstrap_seed=bootstrap_seed,
        comparison_profile=comparison_profile,
    )
    ack_comparison = summarize_scenario_lane(
        candidate_ack,
        baseline_ack,
        lane="command_ack",
        fixture_identity_check=fixture_identity_check,
        bootstrap_resamples=bootstrap_resamples,
        bootstrap_seed=bootstrap_seed,
        comparison_profile=comparison_profile,
    )
    return {
        "prefill_depth": prefill_depth,
        "warmups": warmups,
        "resource_samples": resource_samples,
        "measured_depth_range": [
            prefill_depth + warmups,
            prefill_depth + warmups + samples - 1,
        ],
        "resource_depth_range": [
            prefill_depth + warmups + samples,
            prefill_depth + warmups + samples + resource_samples - 1,
        ]
        if resource_samples
        else None,
        "pair_orders": orders,
        "fixture_identities": fixture_identities,
        "fixture_identity_check": fixture_identity_check,
        "variants": variants,
        "comparisons": {
            "command_ack_ms": ack_comparison,
            "family_sync_fence_ms": material_comparison,
            **durability_comparisons(comparison_profile),
        },
    }
