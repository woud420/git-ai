# Operations

Local development, validation, and release notes for git-ai.

## Local development

- `task dev` — build a debug binary, install it system-wide (same location as
  release builds), run `git-ai install`, and restart the daemon. This is the
  only supported way to run local changes.
- `make check` (bridge for `task lint && task format:check && task test`) —
  the canonical verification.
- `task test TEST_FILTER=foo` / `NO_CAPTURE=true` / `EXTRA_TEST_BINARY_ARGS`,
  `CARGO_TEST_ARGS` — test-suite knobs (see `Taskfile.yml`).
- Coverage: `task coverage` (see `../COVERAGE.md`).

## Daemon

The daemon starts on demand (no launchd/systemd registration). Sockets, locks,
and internal databases live under `~/.git-ai/internal/`. `git-ai daemon`
subcommands and `git-ai status` cover inspection; `GIT_AI_DEBUG=1` enables
debug logging, `GIT_AI_DEBUG_PERFORMANCE=1` timing output.

## Release

Releases are built by `.github/workflows/release.yml` (Linux x64/arm64,
Windows x64/arm64, macOS arm64) and distributed via `install.sh` /
`install.ps1`, Homebrew, and Nix (`flake.nix`, `README-nix.md`).
