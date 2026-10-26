from __future__ import annotations

import os
import subprocess
import sys
import tempfile
import unittest
from argparse import Namespace
from pathlib import Path
from unittest import mock


sys.path.insert(0, str(Path(__file__).parent))
import benchmark_scoped_checkpoint_vs_ref as cli  # noqa: E402
import scoped_checkpoint_contract as benchmark  # noqa: E402
import scoped_checkpoint_runner as runner  # noqa: E402


class StatisticsTests(unittest.TestCase):
    def test_nearest_rank_percentile_uses_observed_values(self) -> None:
        values = [4.0, 1.0, 3.0, 2.0]

        self.assertEqual(benchmark.nearest_rank_percentile(values, 0.50), 2.0)
        self.assertEqual(benchmark.nearest_rank_percentile(values, 0.95), 4.0)

    def test_paired_bootstrap_ratio_is_deterministic(self) -> None:
        candidate = [2.0, 4.0, 6.0, 8.0]
        baseline = [1.0, 2.0, 3.0, 4.0]

        first = benchmark.paired_bootstrap_ratio(
            candidate, baseline, resamples=500, seed=364
        )
        second = benchmark.paired_bootstrap_ratio(
            candidate, baseline, resamples=500, seed=364
        )

        self.assertEqual(first, second)
        self.assertEqual(first[0], 2.0)
        self.assertEqual(first[1:], (2.0, 2.0))

    def test_paired_bootstrap_rejects_zero_baseline(self) -> None:
        with self.assertRaisesRegex(ValueError, "baseline values must be positive"):
            benchmark.paired_bootstrap_ratio([1.0], [0.0], resamples=10, seed=1)

    def test_paired_ratio_uses_within_pair_ratios(self) -> None:
        ratio, _, _ = benchmark.paired_bootstrap_ratio(
            [1.0, 100.0, 1000.0],
            [1.0, 10.0, 1000.0],
            resamples=100,
            seed=364,
        )

        self.assertEqual(ratio, 1.0)


class ContractTests(unittest.TestCase):
    def test_decision_qualification_requires_the_full_measurement_protocol(self) -> None:
        qualified = Namespace(
            samples=30,
            warmups=5,
            resource_samples=20,
            prefill_depths=[0, 50, 200],
            bootstrap_resamples=10_000,
            bootstrap_seed=364,
        )

        self.assertTrue(cli.has_qualified_measurement_protocol(qualified))
        for field, value in {
            "samples": 29,
            "warmups": 4,
            "resource_samples": 19,
            "prefill_depths": [0, 200],
            "bootstrap_resamples": 9_999,
            "bootstrap_seed": 365,
        }.items():
            incomplete = Namespace(**vars(qualified))
            setattr(incomplete, field, value)
            self.assertFalse(cli.has_qualified_measurement_protocol(incomplete), field)

        for field, value in {
            "samples": 31,
            "warmups": 6,
            "resource_samples": 21,
            "prefill_depths": [0, 50, 200, 250],
            "bootstrap_resamples": 10_001,
        }.items():
            different = Namespace(**vars(qualified))
            setattr(different, field, value)
            self.assertFalse(cli.has_qualified_measurement_protocol(different), field)

    def test_decision_qualification_requires_complete_resource_counters(self) -> None:
        def scenario(count: int = 20) -> dict[str, object]:
            variant = {
                "client_resources": {
                    "instructions_retired": {"count": count},
                    "max_rss_bytes": {"count": count},
                },
                "daemon_resources": {
                    "user_seconds": 1.0,
                    "sys_seconds": 0.5,
                    "max_rss_bytes": 1024,
                },
            }
            return {"variants": {"candidate": variant, "baseline": variant}}

        self.assertTrue(cli.has_qualified_resource_evidence([scenario()], 20))
        self.assertFalse(cli.has_qualified_resource_evidence([scenario(19)], 20))
        incomplete = scenario()
        del incomplete["variants"]["candidate"]["daemon_resources"]["max_rss_bytes"]
        self.assertFalse(cli.has_qualified_resource_evidence([incomplete], 20))

    def test_run_protocol_digest_binds_measurement_parameters(self) -> None:
        parameters = Namespace(
            samples=30,
            warmups=5,
            resource_samples=20,
            prefill_depths=[0, 50, 200],
            bootstrap_resamples=10_000,
            bootstrap_seed=364,
            comparison_profile="fork-before-after-v1",
            debug_stages=False,
        )
        changed = Namespace(**vars(parameters))
        changed.samples = 31
        debug_changed = Namespace(**vars(parameters))
        debug_changed.debug_stages = True
        capnp_changed = Namespace(**vars(parameters))
        capnp_changed.checkpoint_capnp = True

        exact = cli.protocol_for_run(parameters)
        different = cli.protocol_for_run(changed)
        debug_different = cli.protocol_for_run(debug_changed)
        capnp_different = cli.protocol_for_run(capnp_changed)

        self.assertNotEqual(
            benchmark.canonical_digest(exact), benchmark.canonical_digest(different)
        )
        self.assertNotEqual(
            benchmark.canonical_digest(exact),
            benchmark.canonical_digest(debug_different),
        )
        self.assertFalse(exact["measurement"]["checkpoint_capnp"])
        self.assertTrue(capnp_different["measurement"]["checkpoint_capnp"])
        self.assertNotEqual(
            benchmark.canonical_digest(exact),
            benchmark.canonical_digest(capnp_different),
        )

    def test_capnp_request_is_bound_in_the_report_manifest(self) -> None:
        args = Namespace(
            samples=30,
            warmups=5,
            resource_samples=20,
            prefill_depths=[0, 50, 200],
            bootstrap_resamples=10_000,
            bootstrap_seed=364,
            comparison_profile="fork-before-after-v1",
            debug_stages=False,
            allow_dirty_sources=False,
            baseline_ref="pinned-baseline",
            checkpoint_capnp=True,
        )
        variants = {"candidate": {}, "baseline": {}}
        readbacks = {
            "candidate": {"status": "enabled", "effective": True},
            "baseline": {"status": "unsupported", "effective": None},
        }
        scenario = {
            "prefill_depth": 0,
            "variants": {
                name: {"checkpoint_capnp_readback": evidence}
                for name, evidence in readbacks.items()
            },
        }
        with mock.patch.object(cli, "file_set_digest", return_value="fixed-harness"):
            result = cli.build_result(
                args=args,
                script_path=Path(cli.__file__),
                git_binary="/usr/bin/git",
                source=variants,
                binary=variants,
                build_provenance={
                    name: {"status": "external_binary_provenance_unverified"}
                    for name in variants
                },
                scenarios=[scenario],
                environment={},
            )
        self.assertTrue(result["parameters"]["checkpoint_capnp"])
        self.assertTrue(result["fixture_contract"]["checkpoint_capnp_requested"])
        self.assertTrue(result["protocol"]["measurement"]["checkpoint_capnp"])
        self.assertEqual(result["qualification"], "smoke_or_diagnostic_non_decision")
        for name, evidence in readbacks.items():
            self.assertEqual(
                result["variants"][name]["checkpoint_capnp_readback_by_depth"],
                [{"prefill_depth": 0, "evidence": evidence}],
            )

    def test_source_snapshot_must_remain_unchanged_through_the_run(self) -> None:
        expected = {
            "commit": "abc",
            "dirty": False,
            "dirty_status": [],
            "cargo_lock_sha256": "lock-a",
        }
        changed = {**expected, "commit": "def"}

        with self.assertRaisesRegex(RuntimeError, "candidate source changed"):
            cli.assert_source_metadata_unchanged("candidate", expected, changed)

    def test_checkpoint_storage_state_counts_only_journals_and_blobs(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            working_logs = Path(directory)
            first = working_logs / "base-a"
            second = working_logs / "base-b"
            (first / "blobs").mkdir(parents=True)
            (second / "blobs").mkdir(parents=True)
            (first / "checkpoints.jsonl").write_bytes(b"abc")
            (second / "checkpoints.jsonl").write_bytes(b"12345")
            (first / "blobs" / "one").write_bytes(b"data")
            (second / "blobs" / "two").write_bytes(b"ok")
            (first / "INITIAL").write_bytes(b"not-counted")
            (first / ".checkpoint-journal.lock").write_bytes(b"not-counted")

            self.assertEqual(
                runner.checkpoint_storage_state(working_logs),
                {
                    "checkpoint_journal_bytes": 8,
                    "blob_count": 2,
                    "blob_bytes": 6,
                    "total_checkpoint_bytes": 14,
                },
            )

    def test_checkpoint_storage_state_is_zero_for_missing_tree(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            self.assertEqual(
                runner.checkpoint_storage_state(Path(directory) / "missing"),
                {
                    "checkpoint_journal_bytes": 0,
                    "blob_count": 0,
                    "blob_bytes": 0,
                    "total_checkpoint_bytes": 0,
                },
            )

    def test_schema_and_protocol_bind_post_shutdown_storage_measurement(self) -> None:
        protocol = cli.protocol_for_profile("fork-before-after-v1")

        self.assertEqual(cli.SCHEMA, "git-ai-scoped-checkpoint-comparison/1.4.0")
        self.assertEqual(
            protocol["checkpoint_storage"]["measurement_phase"],
            "after_daemon_shutdown",
        )
        self.assertEqual(
            protocol["checkpoint_storage"]["timing_effect"],
            "outside latency and process-resource samples",
        )

    def test_variant_uses_dedicated_short_socket_root(self) -> None:
        with tempfile.TemporaryDirectory() as fixture_directory:
            with tempfile.TemporaryDirectory() as socket_directory:
                variant = runner.VariantRunner(
                    label="candidate",
                    binary=Path("/bin/echo"),
                    root=Path(fixture_directory),
                    socket_root=Path(socket_directory),
                    git_binary="/usr/bin/git",
                    debug_stages=False,
                )

                self.assertEqual(
                    variant.control_socket, Path(socket_directory) / "control.sock"
                )
                self.assertEqual(
                    variant.trace_socket, Path(socket_directory) / "trace.sock"
                )

    def test_capnp_environment_is_opt_in_and_ambient_flags_stay_excluded(self) -> None:
        with mock.patch.dict(os.environ, {"GIT_AI_CHECKPOINT_CAPNP": "true"}):
            default = runner.VariantRunner(
                label="candidate",
                binary=Path("/bin/echo"),
                root=Path("/fixture/candidate"),
                socket_root=Path("/sockets/candidate"),
                git_binary="/usr/bin/git",
                debug_stages=False,
            )
            enabled = runner.VariantRunner(
                label="candidate",
                binary=Path("/bin/echo"),
                root=Path("/fixture/candidate"),
                socket_root=Path("/sockets/candidate"),
                git_binary="/usr/bin/git",
                debug_stages=False,
                checkpoint_capnp=True,
            )
        self.assertNotIn("GIT_AI_CHECKPOINT_CAPNP", default.env)
        self.assertEqual(enabled.env["GIT_AI_CHECKPOINT_CAPNP"], "true")
        self.assertEqual(
            {key: value for key, value in enabled.env.items() if key != "GIT_AI_CHECKPOINT_CAPNP"},
            default.env,
        )

    def test_capnp_config_readback_records_enabled_and_unsupported_baseline(self) -> None:
        for label, stdout, expected in (
            ("candidate", '{"checkpoint_capnp":true}', "enabled"),
            ("baseline", '{"transcript_sweep":true}', "unsupported"),
        ):
            with self.subTest(label=label):
                completed = subprocess.CompletedProcess([], 0, stdout=stdout, stderr="")
                with mock.patch.object(runner, "run_checked", return_value=completed) as run:
                    evidence = runner.verify_checkpoint_capnp_config(
                        binary=Path("/fixture/git-ai"), label=label,
                        cwd=Path("/fixture"), env={}, requested=True,
                    )
                self.assertEqual(evidence["status"], expected)
                self.assertEqual(evidence["effective"], True if label == "candidate" else None)
                run.assert_called_once_with(
                    ["/fixture/git-ai", "config", "feature_flags"],
                    cwd=Path("/fixture"), env={},
                )

    def test_requested_capnp_config_rejects_disabled_or_unsupported_candidate(self) -> None:
        for label, stdout in (
            ("candidate", '{"checkpoint_capnp":false}'),
            ("candidate", '{}'),
            ("baseline", '{"checkpoint_capnp":false}'),
        ):
            with self.subTest(label=label, stdout=stdout):
                completed = subprocess.CompletedProcess([], 0, stdout=stdout, stderr="")
                with mock.patch.object(runner, "run_checked", return_value=completed):
                    with self.assertRaisesRegex(RuntimeError, "effective checkpoint_capnp=true"):
                        runner.verify_checkpoint_capnp_config(
                            binary=Path("/fixture/git-ai"), label=label,
                            cwd=Path("/fixture"), env={}, requested=True,
                        )

    def test_capnp_config_readback_rejects_a_nonboolean_flag(self) -> None:
        completed = subprocess.CompletedProcess(
            [], 0, stdout='{"checkpoint_capnp":"true"}', stderr=""
        )
        with mock.patch.object(runner, "run_checked", return_value=completed):
            with self.assertRaisesRegex(RuntimeError, "must be boolean"):
                runner.verify_checkpoint_capnp_config(
                    binary=Path("/fixture/git-ai"), label="candidate",
                    cwd=Path("/fixture"), env={}, requested=True,
                )

    def test_default_transport_does_not_add_a_feature_flag_readback(self) -> None:
        with mock.patch.object(runner, "run_checked") as run:
            evidence = runner.verify_checkpoint_capnp_config(
                binary=Path("/fixture/git-ai"), label="candidate",
                cwd=Path("/fixture"), env={}, requested=False,
            )
        run.assert_not_called()
        self.assertEqual(evidence["status"], "not_requested")

    def test_capnp_qualification_requires_candidate_readback_and_known_baseline_state(self) -> None:
        enabled = {
            "status": "enabled", "requested": True, "supported": True,
            "effective": True, "method": "native-config-feature-flags-readback/v1",
        }
        unsupported = {**enabled, "status": "unsupported", "supported": False, "effective": None}
        scenario = {"variants": {
            "candidate": {"checkpoint_capnp_readback": enabled},
            "baseline": {"checkpoint_capnp_readback": unsupported},
        }}
        self.assertTrue(cli.has_qualified_checkpoint_capnp_evidence([scenario], requested=True))
        self.assertFalse(cli.has_qualified_checkpoint_capnp_evidence([], requested=True))
        self.assertTrue(cli.has_qualified_checkpoint_capnp_evidence([], requested=False))
        for label, invalid in (
            ("candidate", {}), ("candidate", unsupported),
            ("candidate", {**enabled, "effective": False}),
            ("baseline", {**enabled, "effective": False}),
        ):
            changed = {"variants": {name: dict(value) for name, value in scenario["variants"].items()}}
            changed["variants"][label]["checkpoint_capnp_readback"] = invalid
            self.assertFalse(cli.has_qualified_checkpoint_capnp_evidence([changed], requested=True))

    def test_run_scenario_passes_capnp_request_to_both_variants(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with (
                mock.patch.object(runner, "create_short_socket_root", return_value=root / "sockets"),
                mock.patch.object(runner, "VariantRunner") as variants,
            ):
                variants.return_value.setup.side_effect = RuntimeError("stop before setup")
                with self.assertRaisesRegex(RuntimeError, "stop before setup"):
                    runner.run_scenario(
                        scenario_root=root,
                        candidate_binary=Path("/candidate"),
                        baseline_binary=Path("/baseline"),
                        git_binary="/usr/bin/git",
                        prefill_depth=0,
                        warmups=5,
                        samples=30,
                        resource_samples=20,
                        debug_stages=False,
                        bootstrap_resamples=10_000,
                        bootstrap_seed=364,
                        checkpoint_capnp=True,
                    )
        self.assertEqual(len(variants.call_args_list), 2)
        self.assertEqual(
            [call.kwargs["checkpoint_capnp"] for call in variants.call_args_list],
            [True, True],
        )

    def test_matching_capnp_fixture_identities_remain_comparable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            identities = []
            for label, requested in (("candidate", True), ("baseline", True), ("default", False)):
                variant_root = root / label
                (variant_root / "repo" / ".git").mkdir(parents=True)
                (variant_root / "home" / ".git-ai").mkdir(parents=True)
                (variant_root / "repo" / ".git" / "config").write_text("fixed config\n")
                (variant_root / "home" / ".git-ai" / "config.json").write_text("{}\n")
                (variant_root / "repo" / "distractor.txt").write_bytes(b"distractor\n")
                variant = runner.VariantRunner(
                    label=label,
                    binary=Path("/bin/echo"),
                    root=variant_root,
                    socket_root=root / "sockets" / label,
                    git_binary="/usr/bin/git",
                    debug_stages=False,
                    checkpoint_capnp=requested,
                )
                completed = subprocess.CompletedProcess([], 0, stdout="fixed git result\n", stderr="")
                with mock.patch.object(runner, "run_checked", return_value=completed):
                    identities.append(variant.fixture_identity())
        self.assertTrue(identities[0]["checkpoint_capnp_requested"])
        self.assertEqual(
            benchmark.comparison_status(identities[0], identities[1], runner.FIXTURE_IDENTITY_KEYS)["status"],
            "comparable",
        )
        mismatch = benchmark.comparison_status(identities[0], identities[2], runner.FIXTURE_IDENTITY_KEYS)
        self.assertEqual(mismatch["status"], "not_comparable")
        self.assertIn("checkpoint_capnp_requested", mismatch["mismatches"])

    def test_real_harness_file_set_binds_imported_modules(self) -> None:
        paths = cli.harness_files(Path(cli.__file__).resolve())

        self.assertCountEqual(
            [path.name for path in paths],
            [
                "benchmark_scoped_checkpoint_vs_ref.py",
                "scoped_checkpoint_contract.py",
                "scoped_checkpoint_record.py",
                "scoped_checkpoint_runner.py",
                "benchmark_common.py",
            ],
        )

    def test_fixture_config_uses_the_cross_version_allowlist_key(self) -> None:
        config = runner.fixture_git_ai_config("/usr/bin/git")

        self.assertEqual(
            config,
            {
                "allow_repositories": [benchmark.FIXTURE_ALLOWED_REMOTE],
                "git_path": "/usr/bin/git",
            },
        )

    def test_fixture_config_readback_proves_both_effective_values(self) -> None:
        completed = [
            subprocess.CompletedProcess(
                args=[],
                returncode=0,
                stdout=f'["{benchmark.FIXTURE_ALLOWED_REMOTE}"]\n',
                stderr="",
            ),
            subprocess.CompletedProcess(
                args=[], returncode=0, stdout='"/usr/bin/git"\n', stderr=""
            ),
        ]

        with mock.patch.object(runner, "run_checked", side_effect=completed) as run:
            evidence = runner.verify_fixture_git_ai_config(
                binary=Path("/fixture/git-ai"),
                git_binary="/usr/bin/git",
                cwd=Path("/fixture"),
                env={"HOME": "/fixture/home"},
            )

        self.assertEqual(
            [call.args[0] for call in run.call_args_list],
            [
                ["/fixture/git-ai", "config", "allow_repositories"],
                ["/fixture/git-ai", "config", "git_path"],
            ],
        )
        self.assertEqual(
            evidence,
            {
                "status": "verified",
                "method": "native-config-readback/v1",
                "allowlist_config_key": "allow_repositories",
                "allow_repositories": [benchmark.FIXTURE_ALLOWED_REMOTE],
                "git_path": "/usr/bin/git",
            },
        )

    def test_fixture_config_readback_rejects_a_silently_ignored_allowlist(self) -> None:
        ignored = subprocess.CompletedProcess(
            args=[], returncode=0, stdout="[]\n", stderr=""
        )

        with mock.patch.object(runner, "run_checked", return_value=ignored):
            with self.assertRaisesRegex(RuntimeError, "effective allow_repositories"):
                runner.verify_fixture_git_ai_config(
                    binary=Path("/fixture/git-ai"),
                    git_binary="/usr/bin/git",
                    cwd=Path("/fixture"),
                    env={"HOME": "/fixture/home"},
                )

    def test_denial_control_uses_a_mismatched_remote_and_zero_storage(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            denied_repo = root / "denied-repo"
            denied_repo.mkdir()
            variant = runner.VariantRunner(
                label="baseline",
                binary=Path("/fixture/git-ai"),
                root=root,
                socket_root=root / "sockets",
                git_binary="/usr/bin/git",
                debug_stages=False,
            )
            variant.denied_repo = denied_repo

            with (
                mock.patch.object(runner, "run_checked") as run,
                mock.patch.object(variant, "sync_family") as sync,
            ):
                evidence = variant.verify_allowlist_denial()

        run.assert_called_once_with(
            ["/fixture/git-ai", "checkpoint", "mock_ai", "sample.txt"],
            cwd=denied_repo,
            env=variant.env,
        )
        sync.assert_called_once_with(denied_repo)
        self.assertEqual(evidence["status"], "verified_denied")
        self.assertEqual(evidence["allowed_remote"], benchmark.FIXTURE_ALLOWED_REMOTE)
        self.assertEqual(evidence["denied_remote"], benchmark.FIXTURE_DENIED_REMOTE)
        self.assertEqual(evidence["checkpoint_storage"]["total_checkpoint_bytes"], 0)

    def test_protocol_records_the_fresh_daemon_denial_boundary(self) -> None:
        authorization = cli.protocol_for_profile("fork-vs-upstream-v1")[
            "repository_authorization"
        ]

        self.assertEqual(authorization["config_key"], "allow_repositories")
        self.assertEqual(
            authorization["allowed_remote"], benchmark.FIXTURE_ALLOWED_REMOTE
        )
        self.assertEqual(
            authorization["denied_remote"], benchmark.FIXTURE_DENIED_REMOTE
        )
        self.assertEqual(
            authorization["measurement_isolation"],
            "denial daemon stopped before a fresh measurement daemon starts",
        )

    def test_file_set_digest_binds_every_harness_module(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.py"
            second = Path(directory) / "second.py"
            first.write_text("one", encoding="utf-8")
            second.write_text("two", encoding="utf-8")
            original = benchmark.file_set_digest([second, first])

            second.write_text("changed", encoding="utf-8")

            self.assertNotEqual(original, benchmark.file_set_digest([first, second]))

    def test_pair_order_alternates_candidate_and_baseline(self) -> None:
        self.assertEqual(benchmark.pair_order(0), ("candidate", "baseline"))
        self.assertEqual(benchmark.pair_order(1), ("baseline", "candidate"))

    def test_comparison_is_suppressed_when_common_identity_differs(self) -> None:
        candidate = {
            "harness_digest": "same",
            "fixture_digest": "same",
            "config_digest": "same",
            "environment_digest": "environment-a",
            "protocol_digest": "protocol-a",
        }
        baseline = {**candidate, "environment_digest": "environment-b"}

        result = benchmark.comparison_status(candidate, baseline)

        self.assertEqual(result["status"], "not_comparable")
        self.assertIn("environment_digest", result["mismatches"])

    def test_comparison_is_suppressed_when_identity_is_missing(self) -> None:
        result = benchmark.comparison_status({}, {})

        self.assertEqual(result["status"], "not_comparable")
        self.assertCountEqual(result["mismatches"], benchmark.COMMON_IDENTITY_KEYS)

    def test_comparison_is_suppressed_when_protocol_differs(self) -> None:
        candidate = {
            "harness_digest": "same",
            "fixture_digest": "same",
            "config_digest": "same",
            "environment_digest": "same",
            "protocol_digest": "protocol-a",
        }
        baseline = {**candidate, "protocol_digest": "protocol-b"}

        result = benchmark.comparison_status(candidate, baseline)

        self.assertEqual(result["status"], "not_comparable")
        self.assertEqual(result["mismatches"], ["protocol_digest"])

    def test_receipt_lane_defaults_to_contract_incomparable(self) -> None:
        result = benchmark.summarize_cross_variant_lane(
            [30.0, 31.0],
            [4.0, 5.0],
            lane="command_ack",
            bootstrap_resamples=100,
            bootstrap_seed=364,
        )

        self.assertEqual(result["comparison_status"], "not_comparable")
        self.assertNotIn("paired_ratio", result)

    def test_receipt_lane_is_compared_for_shared_fork_contract(self) -> None:
        result = benchmark.summarize_cross_variant_lane(
            [20.0, 22.0],
            [25.0, 24.0],
            lane="command_ack",
            bootstrap_resamples=100,
            bootstrap_seed=365,
            comparison_profile="fork-before-after-v1",
        )

        self.assertEqual(result["comparison_status"], "comparable")
        self.assertEqual(
            result["comparability_basis"],
            "shared fork-live-application/v1 acknowledgement boundary",
        )
        self.assertIn("paired_ratio", result)
        self.assertIn("paired_difference_ms", result)

    def test_scenario_lane_refuses_a_fixture_mismatch_under_shared_ack(self) -> None:
        result = runner.summarize_scenario_lane(
            [20.0, 22.0],
            [25.0, 24.0],
            lane="command_ack",
            fixture_identity_check={
                "status": "not_comparable",
                "mismatches": ["head_commit"],
            },
            bootstrap_resamples=100,
            bootstrap_seed=365,
            comparison_profile="fork-before-after-v1",
        )

        self.assertEqual(result["comparison_status"], "not_comparable")
        self.assertEqual(result["mismatches"], ["head_commit"])
        self.assertNotIn("paired_ratio", result)

    def test_profiles_declare_distinct_and_shared_ack_contracts(self) -> None:
        upstream = benchmark.resolve_comparison_profile("fork-vs-upstream-v1")
        before_after = benchmark.resolve_comparison_profile("fork-before-after-v1")

        self.assertNotEqual(
            upstream["candidate_ack_contract_id"],
            upstream["baseline_ack_contract_id"],
        )
        self.assertEqual(
            before_after["candidate_ack_contract_id"],
            before_after["baseline_ack_contract_id"],
        )

    def test_before_after_durability_claims_are_kept_out_of_latency_lane(self) -> None:
        comparisons = benchmark.durability_comparisons("fork-before-after-v1")

        self.assertEqual(
            comparisons["native_checkpoint_index_durability"]["candidate"],
            "not_assessed_by_latency_harness",
        )
        self.assertEqual(
            comparisons["full_checkpoint_crash_durability"]["candidate"],
            "not_assessed_by_latency_harness",
        )
        self.assertEqual(
            comparisons["full_checkpoint_crash_durability"]["baseline"],
            "unverified",
        )

    def test_cross_project_durability_is_not_inferred_from_latency(self) -> None:
        comparisons = benchmark.durability_comparisons("fork-vs-upstream-v1")

        self.assertEqual(
            comparisons["native_checkpoint_index_durability"]["candidate"],
            "not_assessed_by_latency_harness",
        )
        self.assertEqual(
            comparisons["full_checkpoint_crash_durability"]["candidate"],
            "not_assessed_by_latency_harness",
        )


class CliContractTests(unittest.TestCase):
    def test_cli_defaults_to_fork_vs_upstream_profile(self) -> None:
        argv = [
            "benchmark",
            "--candidate-source",
            "/candidate",
            "--baseline-source",
            "/baseline",
        ]
        with mock.patch.object(sys, "argv", argv):
            args = cli.parse_args()

        self.assertEqual(args.comparison_profile, "fork-vs-upstream-v1")
        self.assertEqual(args.resource_samples, 20)
        self.assertFalse(args.checkpoint_capnp)

    def test_cli_accepts_explicit_capnp_experiment(self) -> None:
        argv = [
            "benchmark", "--candidate-source", "/candidate",
            "--baseline-source", "/baseline", "--checkpoint-capnp",
        ]
        with mock.patch.object(sys, "argv", argv):
            args = cli.parse_args()
        self.assertTrue(args.checkpoint_capnp)

    def test_protocol_digest_changes_with_comparison_profile(self) -> None:
        upstream = cli.protocol_for_profile("fork-vs-upstream-v1")
        before_after = cli.protocol_for_profile("fork-before-after-v1")

        self.assertNotEqual(
            benchmark.canonical_digest(upstream),
            benchmark.canonical_digest(before_after),
        )
        self.assertEqual(
            before_after["candidate_ack_contract_id"],
            before_after["baseline_ack_contract_id"],
        )


class ObservationTests(unittest.TestCase):
    def test_time_metrics_parser_handles_macos_long_format(self) -> None:
        stderr = """
real 0.03
user 0.00
sys 0.00
             2457600  maximum resident set size
            12345678  instructions retired
             8765432  cycles elapsed
              999999  peak memory footprint
"""

        metrics = benchmark.parse_time_metrics(stderr)

        self.assertEqual(metrics["max_rss_bytes"], 2_457_600)
        self.assertEqual(metrics["instructions_retired"], 12_345_678)
        self.assertEqual(metrics["cycles_elapsed"], 8_765_432)
        self.assertEqual(metrics["peak_memory_footprint_bytes"], 999_999)

    def test_cli_stage_parser_extracts_checkpoint_timings(self) -> None:
        stderr = """
[perf] checkpoint: entry_overhead=0.2ms (binary startup + dispatch)
[perf] checkpoint: orchestrator=1.7ms (requests=1, files=1)
[perf] checkpoint: delivery=28.4ms
[perf] checkpoint: total=30.5ms
"""

        self.assertEqual(
            benchmark.parse_cli_stage_metrics(stderr),
            {
                "entry_overhead_ms": 0.2,
                "orchestrator_ms": 1.7,
                "delivery_ms": 28.4,
                "total_ms": 30.5,
            },
        )

if __name__ == "__main__":
    unittest.main()
