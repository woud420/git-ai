# Operations

Local development, validation, and release notes for git-ai.

## Local development

- `make dev` — build a debug binary, install it system-wide (same location as
  release builds), run `git-ai install`, and restart the daemon. This is the
  only supported way to run local changes.
- `make check` — run lint, format checking, and tests sequentially.
- `make test TEST_FILTER=foo` / `NO_CAPTURE=true` / `EXTRA_TEST_BINARY_ARGS`,
  `CARGO_TEST_ARGS` — test-suite knobs (see the root `Makefile`).
- Coverage: `make coverage` (see `../COVERAGE.md`).

## Daemon

The daemon starts on demand (no launchd/systemd registration). Sockets, locks,
and internal databases live under `~/.git-ai/internal/`. `git-ai daemon`
subcommands and `git-ai status` cover inspection; `GIT_AI_DEBUG=1` enables
debug logging, `GIT_AI_DEBUG_PERFORMANCE=1` timing output.

## Release

Release automation is configured in `.github/workflows/release.yml`. It can
build Linux x64/arm64 binaries, Windows x64/arm64 binaries and MSI packages,
macOS Intel and macOS Apple Silicon binaries and PKGs, and a macOS universal
PKG. Production signing and notarization depend on the configured release
environment and secrets.

As of 2026-09-06, this fork has not published tags or release artifacts. Build
and install from source using the root README in the meantime. A successful
non-dry-run workflow can publish version-pinned `install.sh` / `install.ps1`
assets and the platform packages to this fork's GitHub Releases. The Nix flake
can be consumed directly from this repository; no fork-owned Homebrew tap is
currently published.
