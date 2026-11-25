import contextlib
import io
import unittest

from check_test_shards import MAX_POLLS, check_shards

PREFIX = "Test on macos-latest "


def job(name: str, status: str = "completed", conclusion: str = "success") -> dict:
    return {"name": PREFIX + name, "status": status, "conclusion": conclusion}


def run(responses: list[list[dict]], expected: int = 2) -> tuple[int, int, str]:
    """Returns the exit code, the number of API reads, and stderr."""
    reads = []

    def fetch() -> list[dict]:
        reads.append(None)
        return responses[min(len(reads), len(responses)) - 1]

    stderr = io.StringIO()
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(stderr):
        code = check_shards(PREFIX, expected, "macOS", fetch, lambda _: None)
    return code, len(reads), stderr.getvalue()


class CheckShardsTest(unittest.TestCase):
    def test_all_shards_succeeded(self):
        self.assertEqual(run([[job("core"), job("1/1")]])[:2], (0, 1))

    def test_waits_for_a_shard_the_api_still_reports_in_progress(self):
        stale = [job("core"), job("1/1", "in_progress", "")]
        code, reads, _ = run([stale, stale, [job("core"), job("1/1")]])
        self.assertEqual((code, reads), (0, 3))

    def test_failed_shard_fails(self):
        code, reads, stderr = run([[job("core"), job("1/1", conclusion="failure")]])
        self.assertEqual((code, reads), (1, 1))
        self.assertIn("One or more macOS test shards failed", stderr)

    def test_shard_still_running_after_the_last_poll_fails(self):
        stale = [job("core"), job("1/1", "in_progress", "")]
        code, reads, _ = run([stale])
        self.assertEqual((code, reads), (1, MAX_POLLS))

    def test_missing_shard_fails(self):
        code, reads, stderr = run([[job("core")]])
        self.assertEqual((code, reads), (1, MAX_POLLS))
        self.assertIn("Expected 2 jobs", stderr)

    def test_ignores_other_os_jobs(self):
        other = {
            "name": "Test on ubuntu-latest core",
            "status": "queued",
            "conclusion": "",
        }
        self.assertEqual(run([[job("core"), job("1/1"), other]])[:2], (0, 1))


if __name__ == "__main__":
    unittest.main()
