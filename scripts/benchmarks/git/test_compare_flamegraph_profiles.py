from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import compare_flamegraph_profiles as compare  # noqa: E402

PERIOD = 10_101_010  # cpu-clock period at 99 Hz, in nanoseconds


def folded(*stacks: tuple[str, int]) -> str:
    return "\n".join(f"{stack} {samples * PERIOD}" for stack, samples in stacks)


class ParseFoldedTests(unittest.TestCase):
    def test_counts_samples_and_splits_inclusive_from_self_time(self) -> None:
        profile = compare.parse_folded(
            folded(
                ("worker;flush;send", 3),
                ("worker;flush", 1),
                ("worker;<Foo as Bar>::baz", 2),
            )
        )

        self.assertEqual(profile.samples, 6)
        self.assertEqual(profile.total_weight, 6 * PERIOD)
        self.assertEqual(profile.inclusive["worker"], 6 * PERIOD)
        self.assertEqual(profile.inclusive["flush"], 4 * PERIOD)
        self.assertEqual(profile.self_time["flush"], 1 * PERIOD)
        self.assertEqual(profile.self_time["<Foo as Bar>::baz"], 2 * PERIOD)

    def test_recursive_frame_counts_once_per_stack(self) -> None:
        profile = compare.parse_folded(folded(("walk;walk;walk", 2)))

        self.assertEqual(profile.inclusive["walk"], 2 * PERIOD)

    def test_unknown_frames_are_dropped_so_self_time_lands_on_known_code(self) -> None:
        profile = compare.parse_folded(
            folded(("worker;syscall;[unknown];[unknown]", 1))
        )

        self.assertNotIn("[unknown]", profile.inclusive)
        self.assertEqual(profile.self_time["syscall"], PERIOD)

    def test_ignores_blank_and_malformed_lines(self) -> None:
        profile = compare.parse_folded("\nnot-a-count\nworker 5\n")

        self.assertEqual(profile.samples, 1)


class FrameDeltaTests(unittest.TestCase):
    def test_ranks_by_share_change_not_absolute_time(self) -> None:
        # `parse` and `idle` take the same time in both; only `send` grew.
        baseline = compare.parse_folded(
            folded(("w;send", 10), ("w;parse", 11), ("w;idle", 19))
        )
        candidate = compare.parse_folded(
            folded(("w;send", 30), ("w;parse", 11), ("w;idle", 19))
        )

        rows = compare.frame_deltas(baseline, candidate, "self_time", limit=1)

        self.assertEqual(rows[0].frame, "send")
        self.assertAlmostEqual(rows[0].baseline_share, 0.25)
        self.assertAlmostEqual(rows[0].candidate_share, 0.5)
        self.assertGreater(rows[0].z, 0)

    def test_frame_missing_from_one_profile_has_zero_share_there(self) -> None:
        baseline = compare.parse_folded(folded(("w;old_path", 5), ("w;idle", 5)))
        candidate = compare.parse_folded(folded(("w;idle", 10)))

        rows = {
            row.frame: row
            for row in compare.frame_deltas(baseline, candidate, "self_time", 10)
        }

        self.assertAlmostEqual(rows["old_path"].candidate_share, 0.0)
        self.assertLess(rows["old_path"].delta, 0)

    def test_noise_level_change_is_not_significant(self) -> None:
        baseline = compare.parse_folded(folded(("w;a", 500), ("w;b", 500)))
        candidate = compare.parse_folded(folded(("w;a", 510), ("w;b", 490)))

        row = compare.frame_deltas(baseline, candidate, "self_time", 1)[0]

        self.assertLess(abs(row.z), compare.SIGNIFICANT_Z)


class ReportTests(unittest.TestCase):
    def test_report_compares_cpu_time_and_warns_on_few_samples(self) -> None:
        baseline = compare.parse_folded(folded(("w;a|b", 3), ("w;c", 7)))
        candidate = compare.parse_folded(folded(("w;a|b", 7), ("w;c", 8)))

        report = compare.render_report(baseline, candidate, limit=5)

        self.assertIn("| Samples | 10 | 15 | |", report)
        self.assertIn("+50.0%", report)
        self.assertIn("fewer than 1,000 samples", report)
        self.assertIn("`a\\|b`", report)

    def test_empty_profiles_render_without_error(self) -> None:
        empty = compare.parse_folded("")

        report = compare.render_report(empty, empty, limit=5)

        self.assertIn("(no samples)", report)
        self.assertIn("n/a", report)


if __name__ == "__main__":
    unittest.main()
