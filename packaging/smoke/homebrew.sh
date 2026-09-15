#!/usr/bin/env bash
set -euo pipefail

checkout="$(cd "$(dirname "$0")/../.." && pwd -P)"
smoke_dir="$(mktemp -d "${RUNNER_TEMP:-/tmp}/git-ai-homebrew.XXXXXX")"
formula=git-ai-ci/smoke/git-ai
revision="$(git -C "$checkout" rev-parse HEAD)"

# A local source clone keeps --HEAD on the PR revision, including merge commits.
git clone --quiet --no-hardlinks "$checkout" "$smoke_dir/source"
git -C "$smoke_dir/source" checkout --quiet -b package-smoke "$revision"
mkdir -p "$smoke_dir/tap/Formula"
python3 - "$checkout" "$smoke_dir" <<'PY'
import json
import sys
from pathlib import Path

checkout, smoke_dir = map(Path, sys.argv[1:])
source = (checkout / "Formula/git-ai.rb").read_text()
upstream = '"https://github.com/woud420/git-ai.git", branch: "main"'
replacement = f'{json.dumps((smoke_dir / "source").as_uri())}, using: :git, branch: "package-smoke"'
assert source.count(upstream) == 1, "expected exactly one source HEAD URL"
source = source.replace(upstream, replacement)
(smoke_dir / "tap/Formula/git-ai.rb").write_text(source)
PY
git -C "$smoke_dir/tap" init --quiet
git -C "$smoke_dir/tap" add Formula/git-ai.rb
git -C "$smoke_dir/tap" -c user.name=package-smoke -c user.email=package-smoke@example.com \
  commit --quiet -m 'Test current package formula'
brew tap git-ai-ci/smoke "$smoke_dir/tap"
if brew help trust >/dev/null 2>&1; then
  brew trust --formula "$formula"
fi

python3 "$checkout/packaging/smoke/user-state.py" > "$smoke_dir/before.json"
brew install --HEAD "$formula"
python3 "$checkout/packaging/smoke/user-state.py" > "$smoke_dir/after.json"
cmp "$smoke_dir/before.json" "$smoke_dir/after.json"

prefix="$(brew --prefix "$formula")"
test "$(cat "$prefix/bin/git-ai-package-manager")" = homebrew
test ! -e "$prefix/bin/git"
"$prefix/bin/git-ai" --version
brew test "$formula"

upgrade_status=0
"$prefix/bin/git-ai" upgrade --force > "$smoke_dir/upgrade.txt" 2>&1 || upgrade_status=$?
test "$upgrade_status" -eq 1
grep -F 'brew upgrade' "$smoke_dir/upgrade.txt"
"$prefix/bin/git-ai" upgrade --background > "$smoke_dir/background.txt" 2>&1
test ! -s "$smoke_dir/background.txt"

python3 "$checkout/packaging/smoke/user-state.py" > "$smoke_dir/pre-uninstall.json"
brew uninstall "$formula"
test ! -e "$prefix/bin/git-ai"
python3 "$checkout/packaging/smoke/user-state.py" > "$smoke_dir/post-uninstall.json"
cmp "$smoke_dir/pre-uninstall.json" "$smoke_dir/post-uninstall.json"
brew untap git-ai-ci/smoke
