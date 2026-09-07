# git-ai

[![Test](https://github.com/woud420/git-ai/actions/workflows/test.yml/badge.svg)](https://github.com/woud420/git-ai/actions/workflows/test.yml)
[![Lint & Format](https://github.com/woud420/git-ai/actions/workflows/lint-format.yml/badge.svg)](https://github.com/woud420/git-ai/actions/workflows/lint-format.yml)
[![License: Apache-2.0](https://img.shields.io/github/license/woud420/git-ai)](LICENSE)

`git-ai` is a local-first Git extension for explicit, line-level authorship.
Coding agents checkpoint directly; editor integrations may recognize agent-originated edits
from high-confidence event or call-stack signatures. Git-ai connects only that evidence to
agent, model, session, and prompt metadata. Unknown edits remain unknown or untracked;
git-ai does not inspect source content with an AI detector.

Collection is opt-in for each repository through `allowed_repositories`.
Git commands are observed through Trace2 and processed asynchronously by a
per-repository daemon; attribution work does not run in Git's critical path.

## Features

- Inspect committed attribution with `git ai log`, `blame`, `diff`, `stats`,
  and `show`; inspect current work with `status` and local activity with
  `usage`.
- Checkpoint presets cover multiple coding agents and preserve agent, model,
  session, trace, and prompt links.
- Known-human attribution is recorded only when an editor integration provides
  evidence. Lines without attribution evidence remain unknown or untracked.
- Authorship is preserved across supported rebases, amends, cherry-picks,
  squash merges, resets, stashes, and reverts. Ambiguous rewrites fail closed.
- The default local backend stores authorship notes in SQLite. The optional
  `git_notes` backend stores shareable notes in `refs/notes/ai`, with explicit
  migration between backends.
- Authorship records use the `authorship/3.0.0` serialization format defined by
  the [upstream Git AI standard](specs/git_ai_standard_v3.0.0.md). Its Git Notes
  storage profile applies only to the opt-in `git_notes` backend; the default
  SQLite backend follows this fork's [persistence contract](docs/contracts/persistence-model.md).

## Install and quick start

Until this fork publishes release artifacts, build and install from source.
The installers support macOS and Linux on x86_64 or ARM64, and Windows on x64
or ARM64. Install Rust 1.93.0 or newer and Git first, then clone this
repository and run the commands for your platform. Do not use `sudo` or an
elevated Windows shell.

### macOS, Linux, or Windows Subsystem for Linux

```bash
cargo build --release --bin git-ai
GIT_AI_LOCAL_BINARY="$PWD/target/release/git-ai" ./install.sh
```

### Windows PowerShell

```powershell
cargo build --release --bin git-ai
$env:GIT_AI_LOCAL_BINARY = (Resolve-Path .\target\release\git-ai.exe)
powershell -NoProfile -ExecutionPolicy Bypass -File .\install.ps1
```

The installer places `git-ai` on your user path and configures supported agent
integrations. Open a new shell if needed, enter a repository, and opt it in:

```bash
git-ai config --add allowed_repositories "$PWD"
```

Work with your configured coding agent and commit normally. Attribution is
processed asynchronously. These commands inspect the result:

```bash
git ai status
git ai stats HEAD
git ai blame path/to/file
```

## Supported workflows and limits

- Platforms: macOS and Linux on x86_64 or ARM64, and Windows on x64 or ARM64.
- Checkpoint presets accept hook data from Claude Code, Cline, Codex, Gemini,
  Windsurf, Continue CLI, Cursor (including background agents), GitHub Copilot,
  Amp, AI Tab, Firebender, Droid, OpenCode, Pi, and compatible `agent-v1` clients.
- Git workflows include normal commits and merges, amend, rebase and
  `pull --rebase`, cherry-pick, squash merge, reset, stash, revert, and
  `commit-tree`/`update-ref` restacks.

Support is evidence-based: repositories outside `allowed_repositories` are
ignored, edits without checkpoint evidence remain unattributed, and ambiguous
rewrites fail closed. See the [checkpoint interface](docs/contracts/checkpoint-interface.md)
and [rewrite operations spec](docs/architecture/rewrite-ops-spec.md) for the
exact contracts and limitations.

## Uninstall

```bash
git-ai uninstall
git-ai uninstall --purge
```

The default command removes agent hooks, the global Trace2 configuration, the
daemon, installed binaries and shims, and installer-added PATH entries while
keeping configuration and local attribution databases. `--purge` also removes
`~/.git-ai`. Neither mode removes repository-local `.git/ai` directories.

## Where to start

- Users: follow [Install and quick start](#install-and-quick-start).
- Contributors: read [CONTRIBUTING.md](CONTRIBUTING.md), then use `make build`,
  `make test`, `make lint`, and `make format-check`.
- Architecture: see [docs/architecture/README.md](docs/architecture/README.md).
- Stable interfaces: see [docs/contracts/README.md](docs/contracts/README.md).
- Authorship serialization format: read
  [specs/git_ai_standard_v3.0.0.md](specs/git_ai_standard_v3.0.0.md).

## Origin, thanks, and license

This repository began as a fork of the original open-source Git AI codebase.
Thanks to Aidan Cunniffe, Sasha Varlamov, and the original contributors for
creating that foundation and publishing the authorship format. This fork is
maintained independently because its interface, storage choices, rewrite
behavior, and contributor workflow have diverged. It continues to use the
`authorship/3.0.0` serialization format defined by the
[upstream standard](specs/git_ai_standard_v3.0.0.md), while that standard's Git
Notes storage profile applies only when this fork uses the `git_notes` backend.

Licensed under [Apache License 2.0](LICENSE).
