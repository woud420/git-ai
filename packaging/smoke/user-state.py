"""Snapshot integration files so package operations cannot silently run user setup."""

import hashlib
import json
from pathlib import Path


paths = (
    ".gitconfig",
    ".git-ai",
    ".claude",
    ".codex",
    ".gemini",
    ".config/git/config",
    ".config/opencode",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".config/fish/config.fish",
)
user_root = Path.home()
state = {}
for relative in paths:
    path = user_root / relative
    entries = [path]
    if path.is_dir() and not path.is_symlink():
        entries.extend(sorted(path.rglob("*")))
    for entry in entries:
        name = str(entry.relative_to(user_root))
        if entry.is_symlink():
            state[name] = f"link:{entry.readlink()}"
        elif entry.is_file():
            state[name] = hashlib.sha256(entry.read_bytes()).hexdigest()
        elif entry.is_dir():
            state[name] = "directory"
print(json.dumps(state, sort_keys=True))
