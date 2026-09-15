#!/usr/bin/env python3
"""Build Homebrew and Chocolatey release assets from verified release binaries."""

import argparse
import hashlib
from pathlib import Path
import re
import zipfile


ROOT = Path(__file__).resolve().parent
REPOSITORY = "woud420/git-ai"
ASSETS = (
    "git-ai-macos-x64",
    "git-ai-macos-arm64",
    "git-ai-linux-x64",
    "git-ai-linux-arm64",
    "git-ai-windows-x64.exe",
    "git-ai-windows-arm64.exe",
)


def verified_assets(directory):
    checksums = {}
    for number, line in enumerate((directory / "SHA256SUMS").read_text().splitlines(), 1):
        match = re.fullmatch(r"([a-fA-F0-9]{64}) [ *]([A-Za-z0-9][A-Za-z0-9._-]*)", line)
        if not match:
            raise ValueError(f"invalid checksum line {number}")
        digest, name = match.groups()
        if name in checksums:
            raise ValueError(f"duplicate checksum: {name}")
        checksums[name] = digest.lower()

    binaries = {}
    for name in ASSETS:
        if name not in checksums:
            raise ValueError(f"missing checksum: {name}")
        content = (directory / name).read_bytes()
        if hashlib.sha256(content).hexdigest() != checksums[name]:
            raise ValueError(f"checksum mismatch: {name}")
        binaries[name] = content
    return checksums, binaries


def render(name, replacements):
    content = (ROOT / "templates" / name).read_text(encoding="utf-8")
    for key, value in replacements.items():
        content = content.replace(f"@{key}@", value)
    if re.search(r"@[A-Z0-9_]+@", content):
        raise ValueError(f"unresolved template variable: {name}")
    return content.encode("utf-8")


def write_nupkg(destination, files):
    with zipfile.ZipFile(destination, "w", compression=zipfile.ZIP_DEFLATED) as package:
        for name, content in sorted(files.items()):
            # Fixed metadata keeps packages reproducible across release runners.
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.create_system = 3
            info.external_attr = 0o100644 << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            package.writestr(info, content)


def generate(args):
    if args.repository != REPOSITORY:
        raise ValueError(f"unsupported repository; expected {REPOSITORY}")
    if not re.fullmatch(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)", args.version):
        raise ValueError("expected a stable version in MAJOR.MINOR.PATCH format")
    if any(int(component) > 2_147_483_647 for component in args.version.split(".")):
        raise ValueError("stable version components must fit NuGet's 32-bit integer fields")
    if args.tag != f"v{args.version}":
        raise ValueError("tag must match v<version>")

    checksums, binaries = verified_assets(args.assets_dir)
    replacements = {"VERSION": args.version, "TAG": args.tag, "REPOSITORY": args.repository}
    for name in ASSETS:
        key = name.removeprefix("git-ai-").removesuffix(".exe").replace("-", "_").upper()
        replacements[f"SHA256_{key}"] = checksums[name]

    formula = render("git-ai.rb.in", replacements)
    package_files = {
        "git-ai.nuspec": render("git-ai.nuspec.in", replacements),
        "[Content_Types].xml": (ROOT / "templates" / "content-types.xml").read_bytes(),
        "_rels/.rels": (ROOT / "templates" / "relationships.xml").read_bytes(),
        "tools/chocolateyInstall.ps1": render("chocolateyInstall.ps1.in", replacements),
        "tools/git-ai-package-manager": b"chocolatey\n",
        "tools/LICENSE.txt": (ROOT.parent.parent / "LICENSE").read_bytes(),
    }
    for arch in ("x64", "arm64"):
        name = f"tools/payload/{arch}/git-ai.exe"
        package_files[name] = binaries[f"git-ai-windows-{arch}.exe"]
        package_files[f"{name}.ignore"] = b""

    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "git-ai.rb").write_bytes(formula)
    write_nupkg(args.output_dir / f"git-ai.{args.version}.nupkg", package_files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", default=REPOSITORY)
    parser.add_argument("--version", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--assets-dir", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    try:
        generate(args)
    except (OSError, ValueError) as error:
        parser.exit(1, f"package generation failed: {error}\n")


if __name__ == "__main__":
    main()
