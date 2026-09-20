# Operations

Local development, validation, and release notes for git-ai.

For explicit offline authorship exchange, see [companion notes bundles](notes-bundle.md).

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

For opt-in agent sandbox access to the trace socket, see
[sandbox socket permissions](sandbox-socket-permissions.md).
See [socket health checks](socket-health.md) for liveness warnings and limits.

Whole-file JSON transcripts (Amp, Continue, and Copilot) have a 64 MiB read
limit. Oversized files remain pending without advancing their transcript
cursor. To admit a larger file, set a positive byte limit with
`git-ai config set max_transcript_file_bytes 134217728` and run
`git-ai bg restart`. `GIT_AI_MAX_TRANSCRIPT_FILE_BYTES` overrides the file
setting. The limit also applies to transcript model probes; it bounds input
bytes, not the memory used by parsed JSON or the daemon as a whole.

Metrics flushes claim at most `max_metrics_flush_chunk_bytes` of queued JSON
per batch (8 MiB by default), before parsing or uploading. This applies to
both the daemon and `git-ai flush-metrics-db`. A single record above the limit
stays pending without a processing lock or failed-upload attempt; it can
block later records and cause `git-ai await` to time out. Increase the positive
limit with `git-ai config set max_metrics_flush_chunk_bytes 16777216` and run
`git-ai bg restart` to resume. `GIT_AI_MAX_METRICS_FLUSH_CHUNK_BYTES` overrides
the file setting. This bounds stored JSON bytes per read, not total RSS or the
serialized HTTP envelope size.

## Git transport

[Standalone send-pack notes export](send-pack-notes.md) documents the opt-in,
bounded metadata profile and its unsupported forms.

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
