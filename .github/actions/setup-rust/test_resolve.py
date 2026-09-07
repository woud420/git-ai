import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = pathlib.Path(__file__).with_name("resolve.sh")
# Avoid CreateProcess preferring Windows' WSL launcher over Git Bash on PATH.
BASH = shutil.which("bash")


class ResolveToolchainTests(unittest.TestCase):
    def resolve(self, mode, manifest=""):
        self.assertIsNotNone(BASH, "Git Bash must be available on PATH")
        with tempfile.TemporaryDirectory(prefix="rust-toolchain-") as directory:
            path = pathlib.Path(directory) / "Cargo.toml"
            path.write_bytes(manifest.encode())
            # Match the action's quoting boundary even for Windows newline inputs.
            return subprocess.run(
                [BASH, "-c", 'source "$RESOLVER_SCRIPT" "$TOOLCHAIN_POLICY" "$TEST_MANIFEST"'],
                env={**os.environ, "RESOLVER_SCRIPT": SCRIPT.as_posix(),
                     "TOOLCHAIN_POLICY": mode, "TEST_MANIFEST": path.as_posix()},
                capture_output=True, text=True, check=False,
            )

    def test_stable_does_not_require_a_manifest(self):
        result = self.resolve("stable")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "stable\n")

    def test_literal_package_minimum(self):
        for declared, expected in [("1.93", "1.93.0"), ("1.94.2", "1.94.2")]:
            for quote in ['"', "'"]:
                for newline in ["\n", "\r\n"]:
                    with self.subTest(declared=declared, quote=quote, newline=newline):
                        manifest = newline.join([
                            '# rust-version = "9.99"',
                            "[package] # source of truth",
                            f"  rust-version = {quote}{declared}{quote} # minimum",
                            "[dependencies]",
                            'rust-version = "9.99"',
                        ])
                        result = self.resolve("msrv", manifest)
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(result.stdout, expected + "\n")

    def test_rejects_missing_ambiguous_or_unsafe_minimum(self):
        for manifest in [
            "", '[dependencies]\nrust-version = "1.93"',
            '[package]\nrust-version = "1.93"\nrust-version = "1.94"',
            '[package]\nrust-version = "1.93" # minimum\nrust-version = "1.94"',
            '[package]\nrust-version.workspace = true',
            '[package]\nrust-version = "stable"',
            '[package]\nrust-version = "1.93; echo unsafe"',
            '[package]\nrust-version = "1.93\' ',
            '[package]\nrust-version = "1.93" trailing-garbage',
        ]:
            with self.subTest(manifest=manifest):
                result = self.resolve("msrv", manifest)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("rust-version", result.stderr)

    def test_rejects_unknown_or_missing_selection(self):
        for mode in ["", "nightly", "1.93.0", "stable\nmsrv", "--help"]:
            with self.subTest(mode=mode):
                result = self.resolve(mode)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
                self.assertIn("stable or msrv", result.stderr)


if __name__ == "__main__":
    unittest.main()
