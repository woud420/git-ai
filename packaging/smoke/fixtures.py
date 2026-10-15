"""Build smoke-only release inputs; unused architectures deliberately cannot run."""

import argparse
import hashlib
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--architecture", choices=("x64", "arm64"), required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    binary = args.binary.read_bytes()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    checksums = []
    for platform in ("linux", "macos", "windows"):
        for architecture in ("x64", "arm64"):
            name = f"git-ai-{platform}-{architecture}"
            if platform == "windows":
                name += ".exe"
            content = (
                binary
                if platform == "windows" and architecture == args.architecture
                else f"SMOKE ONLY: no executable for {name}\n".encode()
            )
            (args.output_dir / name).write_bytes(content)
            checksums.append(f"{hashlib.sha256(content).hexdigest()}  {name}\n")
    (args.output_dir / "SHA256SUMS").write_text("".join(sorted(checksums)))


if __name__ == "__main__":
    main()
