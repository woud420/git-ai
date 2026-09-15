import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile


SCRIPT = Path(__file__).resolve().parents[1] / "generate.py"
ASSETS = (
    "git-ai-macos-x64",
    "git-ai-macos-arm64",
    "git-ai-linux-x64",
    "git-ai-linux-arm64",
    "git-ai-windows-x64.exe",
    "git-ai-windows-arm64.exe",
)


class PackageGenerationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.assets = self.root / "assets"
        self.output = self.root / "output"
        self.assets.mkdir()
        self.contents = {name: f"release fixture: {name}\n".encode() for name in ASSETS}
        for name, content in self.contents.items():
            (self.assets / name).write_bytes(content)
        self.checksums = {
            name: hashlib.sha256(content).hexdigest()
            for name, content in self.contents.items()
        }
        self.write_manifest()

    def write_manifest(self, entries=None):
        entries = self.checksums if entries is None else entries
        (self.assets / "SHA256SUMS").write_text(
            "".join(f"{digest}  {name}\n" for name, digest in entries.items()),
            encoding="utf-8",
        )

    def generate(self, **overrides):
        options = {
            "repository": "woud420/git-ai",
            "version": "1.2.3",
            "tag": "v1.2.3",
            "assets-dir": self.assets,
            "output-dir": self.output,
        }
        options.update(overrides)
        return subprocess.run(
            [sys.executable, str(SCRIPT)]
            + [str(value) for key, item in options.items() for value in (f"--{key}", item)],
            capture_output=True,
            text=True,
        )

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            {path.name for path in self.output.iterdir()},
            {"git-ai.rb", "git-ai.1.2.3.nupkg"},
        )

    def assert_rejected(self, result, message):
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(message, result.stderr)
        self.assertFalse(self.output.exists())

    def test_formula_covers_all_unix_assets_with_exact_release_checksums(self):
        self.assert_success(self.generate())
        formula = (self.output / "git-ai.rb").read_text()
        self.assertIn('version "1.2.3"', formula)
        self.assertIn('license "Apache-2.0"', formula)
        for name in ASSETS[:4]:
            self.assertIn(f"woud420/git-ai/releases/download/v1.2.3/{name}", formula)
            self.assertIn(self.checksums[name], formula)
        self.assertEqual(formula.count("using: :nounzip"), 4)
        self.assertIn('(bin/"git-ai-package-manager").write "homebrew\\n"', formula)
        self.assertIn('shell_output("#{bin}/git-ai --version")', formula)
        self.assertIn('shell_output("#{bin}/git-ai upgrade 2>&1", 1)', formula)
        self.assertIn('head do', formula)
        self.assertIn('url "https://github.com/woud420/git-ai.git", branch: "main"', formula)
        self.assertIn('if build.head?', formula)
        self.assertNotRegex(formula, r"system .*git-ai.*install-hooks|post_install|service do")

    def test_chocolatey_embeds_both_binaries_with_ignored_source_shims(self):
        self.assert_success(self.generate())
        with zipfile.ZipFile(self.output / "git-ai.1.2.3.nupkg") as package:
            for arch in ("x64", "arm64"):
                path = f"tools/payload/{arch}/git-ai.exe"
                self.assertEqual(package.read(path), self.contents[f"git-ai-windows-{arch}.exe"])
                self.assertEqual(package.read(f"{path}.ignore"), b"")
            self.assertNotIn("tools/git-ai.exe", package.namelist())
            self.assertEqual(package.read("tools/git-ai-package-manager"), b"chocolatey\n")
            self.assertEqual(
                {name for name in package.namelist() if name.endswith(".exe")},
                {"tools/payload/x64/git-ai.exe", "tools/payload/arm64/git-ai.exe"},
            )
            script = package.read("tools/chocolateyInstall.ps1").decode()
            self.assertIn("Get-CimInstance", script)
            self.assertIn("Win32_Processor", script)
            self.assertIn("Is64BitOperatingSystem", script)
            self.assertIn("[IO.File]::OpenRead($source)", script)
            self.assertIn("ComputeHash($stream)", script)
            self.assertNotRegex(script, r"Get-FileHash|ReadAllBytes|ToHexString")
            for name in ASSETS[4:]:
                self.assertIn(self.checksums[name], script)
            self.assertNotRegex(script, r"https?://|Install-Chocolatey|& .*install-hooks|Start-Process")

    def test_nupkg_has_parseable_metadata_license_and_opc_relationships(self):
        self.assert_success(self.generate())
        with zipfile.ZipFile(self.output / "git-ai.1.2.3.nupkg") as package:
            metadata = ET.fromstring(package.read("git-ai.nuspec"))
            self.assertEqual(metadata.findtext(".//{*}id"), "git-ai")
            self.assertEqual(metadata.findtext(".//{*}version"), "1.2.3")
            self.assertEqual(metadata.findtext(".//{*}licenseUrl"),
                             "https://github.com/woud420/git-ai/blob/v1.2.3/LICENSE")
            self.assertEqual(metadata.findtext(".//{*}requireLicenseAcceptance"), "false")
            self.assertIn(b"Apache License", package.read("tools/LICENSE.txt"))
            relations = ET.fromstring(package.read("_rels/.rels"))
            for relation in relations:
                self.assertIn(relation.attrib["Target"].lstrip("/"), package.namelist())
            ET.fromstring(package.read("[Content_Types].xml"))
            self.assertIsNone(package.testzip())

    def test_outputs_are_deterministic_across_input_order_and_timestamps(self):
        self.assert_success(self.generate())
        expected = {path.name: path.read_bytes() for path in self.output.iterdir()}
        self.write_manifest(dict(reversed(list(self.checksums.items()))))
        for path in self.assets.iterdir():
            os.utime(path, (1_700_000_000, 1_700_000_000))
        self.assert_success(self.generate())
        self.assertEqual(expected, {path.name: path.read_bytes() for path in self.output.iterdir()})

    def test_additional_release_artifacts_are_not_packaged(self):
        checksums = dict(self.checksums, **{"git-ai-windows-x64.msi": "0" * 64})
        self.write_manifest(checksums)
        self.assert_success(self.generate())
        with zipfile.ZipFile(self.output / "git-ai.1.2.3.nupkg") as package:
            self.assertFalse(any(name.endswith(".msi") for name in package.namelist()))

    def test_every_asset_is_required(self):
        for name in ASSETS:
            with self.subTest(name=name):
                path = self.assets / name
                path.unlink()
                self.assert_rejected(self.generate(), name)
                path.write_bytes(self.contents[name])

    def test_each_asset_must_match_its_checksum(self):
        for name in ASSETS:
            with self.subTest(name=name):
                (self.assets / name).write_bytes(b"changed after release checksum")
                self.assert_rejected(self.generate(), f"checksum mismatch: {name}")
                (self.assets / name).write_bytes(self.contents[name])

    def test_missing_checksum_fails_before_creating_outputs(self):
        self.write_manifest({name: value for name, value in self.checksums.items() if name != ASSETS[0]})
        self.assert_rejected(self.generate(), f"missing checksum: {ASSETS[0]}")

    def test_duplicate_checksum_is_rejected_even_if_identical(self):
        with (self.assets / "SHA256SUMS").open("a") as manifest:
            manifest.write(f"{self.checksums[ASSETS[0]]}  {ASSETS[0]}\n")
        self.assert_rejected(self.generate(), "duplicate checksum")

    def test_malformed_manifest_is_rejected(self):
        for line in ("broken", f"{'0' * 64}  ../git-ai", f"{'g' * 64}  git-ai"):
            with self.subTest(line=line):
                (self.assets / "SHA256SUMS").write_text(f"{line}\n")
                self.assert_rejected(self.generate(), "invalid checksum line")

    def test_binary_mode_checksum_manifest_is_supported(self):
        self.write_manifest()
        manifest = self.assets / "SHA256SUMS"
        manifest.write_text(manifest.read_text().replace("  git-ai", " *git-ai"))
        self.assert_success(self.generate())

    def test_rejects_other_repositories_and_unstable_or_unsafe_versions(self):
        for version in ("v1.2.3", "1.2", "01.2.3", "1.2.3-rc1", "1.2.3+build", "1.2.3\n", "../1.2.3"):
            with self.subTest(version=version):
                self.assert_rejected(self.generate(version=version, tag=f"v{version}"), "stable version")
        self.assert_rejected(self.generate(repository="git-ai-project/git-ai"), "repository")
        self.assert_rejected(self.generate(tag="latest"), "tag must match")


if __name__ == "__main__":
    unittest.main()
