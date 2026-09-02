from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path


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

    def test_real_harness_file_set_binds_imported_modules(self) -> None:
        paths = cli.harness_files(Path(cli.__file__).resolve())

        self.assertCountEqual(
            [path.name for path in paths],
            [
                "benchmark_scoped_checkpoint_vs_ref.py",
                "scoped_checkpoint_contract.py",
                "scoped_checkpoint_runner.py",
                "benchmark_common.py",
            ],
        )

    def test_fixture_config_pins_real_git_binary(self) -> None:
        config = runner.fixture_git_ai_config(Path("/fixture/repo"), "/usr/bin/git")

        self.assertEqual(
            config,
            {
                "allowed_repositories": ["/fixture/repo"],
                "git_path": "/usr/bin/git",
            },
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
        }
        baseline = {**candidate, "environment_digest": "environment-b"}

        result = benchmark.comparison_status(candidate, baseline)

        self.assertEqual(result["status"], "not_comparable")
        self.assertIn("environment_digest", result["mismatches"])

    def test_comparison_is_suppressed_when_identity_is_missing(self) -> None:
        result = benchmark.comparison_status({}, {})

        self.assertEqual(result["status"], "not_comparable")
        self.assertCountEqual(result["mismatches"], benchmark.COMMON_IDENTITY_KEYS)

    def test_receipt_lane_is_always_labeled_contract_incomparable(self) -> None:
        result = benchmark.summarize_cross_variant_lane(
            [30.0, 31.0],
            [4.0, 5.0],
            lane="command_ack",
            bootstrap_resamples=100,
            bootstrap_seed=364,
        )

        self.assertEqual(result["comparison_status"], "not_comparable")
        self.assertNotIn("paired_ratio", result)


class ObservationTests(unittest.TestCase):
    def test_materialized_checkpoint_requires_one_expected_blob_and_path(self) -> None:
        expected = {
            "kind": "AiAgent",
            "entries": [{"file": "sample.txt", "blob_sha": "abc123"}],
            "trace_id": "trace-1",
        }
        lines = [json.dumps(expected), json.dumps({"kind": "Human", "entries": []})]

        record = benchmark.find_materialized_checkpoint(
            lines, expected_blob_sha="abc123", expected_path="sample.txt"
        )

        self.assertEqual(record["trace_id"], "trace-1")

    def test_materialized_checkpoint_rejects_duplicates(self) -> None:
        record = json.dumps(
            {
                "kind": "AiAgent",
                "entries": [{"file": "sample.txt", "blob_sha": "abc123"}],
            }
        )

        with self.assertRaisesRegex(RuntimeError, "exactly one"):
            benchmark.find_materialized_checkpoint(
                [record, record], expected_blob_sha="abc123", expected_path="sample.txt"
            )

    def test_materialized_checkpoint_rejects_extra_entries(self) -> None:
        record = json.dumps(
            {
                "kind": "AiAgent",
                "entries": [
                    {"file": "sample.txt", "blob_sha": "abc123"},
                    {"file": "distractor.txt", "blob_sha": "other"},
                ],
            }
        )

        with self.assertRaisesRegex(RuntimeError, "exactly one"):
            benchmark.find_materialized_checkpoint(
                [record], expected_blob_sha="abc123", expected_path="sample.txt"
            )

    def test_materialized_blob_requires_exact_content(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            working_logs = Path(directory)
            blob_dir = working_logs / "base" / "blobs"
            blob_dir.mkdir(parents=True)
            content = b"expected\n"
            digest = benchmark.sha256_bytes(content)
            (blob_dir / digest).write_bytes(content)

            path = benchmark.validate_materialized_blob(
                working_logs, expected_blob_sha=digest, expected_content=content
            )

            self.assertEqual(path.name, digest)

    def test_materialized_blob_rejects_wrong_content(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            working_logs = Path(directory)
            blob_dir = working_logs / "base" / "blobs"
            blob_dir.mkdir(parents=True)
            content = b"expected\n"
            digest = benchmark.sha256_bytes(content)
            (blob_dir / digest).write_bytes(b"wrong\n")

            with self.assertRaisesRegex(RuntimeError, "content mismatch"):
                benchmark.validate_materialized_blob(
                    working_logs, expected_blob_sha=digest, expected_content=content
                )

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
