#!/usr/bin/env bash
# Opt a CI test repository into git-ai collection.
# Usage: configure-test-repo.sh <git-ai-binary> <repo-dir>
#
# Collection is opt-in: with an empty allowed_repositories list git-ai skips
# checkpoints and the daemon writes no authorship notes. Release builds also
# default to the SQLite notes backend, so pin git_notes for the nightly scripts
# that read 'git notes --ref=ai show HEAD' directly.
# SQLite-default coverage lives in tests/integration/sqlite_notes_backend.rs.
set -euo pipefail

GIT_AI_BIN="${1:?Usage: $0 <git-ai-binary> <repo-dir>}"
REPO_DIR="${2:?Usage: $0 <git-ai-binary> <repo-dir>}"

"$GIT_AI_BIN" config set notes_backend.kind git_notes
# Resolve the canonical path — on macOS /tmp is a symlink to /private/tmp
# and allowed_repositories matching canonicalises the repo root.
"$GIT_AI_BIN" config --add allowed_repositories "$(cd "$REPO_DIR" && pwd -P)"
