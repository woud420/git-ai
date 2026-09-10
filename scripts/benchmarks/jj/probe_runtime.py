#!/usr/bin/env python3
"""Probe jj observation semantics in temporary repositories; never install anything."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--jj", required=True, type=Path, help="Explicit jj binary path")
parser.add_argument("--git", required=True, type=Path, help="Explicit real Git binary path")
parser.add_argument("--output", required=True, type=Path, help="New sanitized JSON evidence file")
args = parser.parse_args()
jj_binary = args.jj.resolve(strict=True)
git_binary = args.git.resolve(strict=True)
if args.output.exists():
    parser.error("--output must not exist")

env = {key: value for key, value in os.environ.items() if not key.startswith(("GIT_", "JJ_"))}
env.update(JJ_CONFIG="", JJ_USER="Probe", JJ_EMAIL="probe@example.invalid",
           GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT="0")
records = []
safe = ["--ignore-working-copy", "--at-operation=@"]
identity = 'commit_id ++ "\\n" ++ change_id ++ "\\n" ++ json(parents.map(|p| p.commit_id()))'

with tempfile.TemporaryDirectory(prefix="git-ai-jj-probe-") as temporary:
    base = Path(temporary).resolve()

    def clean(value):
        return str(value).replace(str(base), "{probe_dir}").replace(str(jj_binary), "{jj}").replace(str(git_binary), "{git}")

    def run(binary, command, cwd, extra=None, expected=0):
        result = subprocess.run([str(binary), *command], cwd=cwd, env=env | (extra or {}),
                                text=True, capture_output=True, timeout=30)
        records.append(dict(command=[clean(binary), *map(clean, command)], cwd=clean(cwd),
                            code=result.returncode, stdout=clean(result.stdout), stderr=clean(result.stderr)))
        assert result.returncode == expected, records[-1]
        return result.stdout.strip()

    def jj(command, cwd, extra=None, expected=0):
        return run(jj_binary, ["--no-pager", "--color=never", *command], cwd, extra, expected)

    def digest(root):
        result = hashlib.sha256()
        for path in sorted(root.rglob("*")):
            if path.is_file():
                name = str(path.relative_to(root)).encode()
                data = path.read_bytes()
                result.update(len(name).to_bytes(8, "big") + name)
                result.update(len(data).to_bytes(8, "big") + data)
        return result.hexdigest()

    def fact(name, **values):
        records.append(dict(probe=name, **values))

    fact("environment", system=platform.system(), architecture=platform.machine(),
         jj_version=jj(["--version"], base), binary_sha256=hashlib.sha256(jj_binary.read_bytes()).hexdigest())
    for mode in ("colocated", "standalone"):
        repo = base / mode
        jj(["git", "init", "--colocate" if mode == "colocated" else "--no-colocate", str(repo)], base)
        (repo / "nested").mkdir()
        (repo / "file.txt").write_text("alpha\n")
        trace = base / f"{mode}.trace"
        jj(["commit", "-m", "first"], repo, {"GIT_TRACE2_EVENT": str(trace)})
        fact(f"{mode}-jj-commit-trace", exists=trace.exists(), bytes=trace.stat().st_size if trace.exists() else 0)
        (repo / "file.txt").write_text("alpha\nbeta-unsnapshotted\n")
        before = digest(repo)
        jj([*safe, "root"], repo / "nested")
        operation = jj([*safe, "op", "log", "-G", "-n", "1", "-T", "id"], repo)
        pinned = ["--ignore-working-copy", f"--at-operation={operation}"]
        original = jj([*pinned, "log", "-G", "-r", "@", "-T", identity], repo)
        jj([*pinned, "log", "-G", "-r", "@", "-T", "json(working_copies.map(|w| w.name()))"], repo)
        jj([*pinned, "workspace", "list", "-T", 'json(name) ++ "\\t" ++ json(root) ++ "\\n"'], repo)
        after = digest(repo)
        fact(f"{mode}-read-only", before=before, after=after, unchanged=before == after)
        assert before == after
        jj(["status"], repo)
        current = jj([*safe, "log", "-G", "-r", "@", "-T", identity], repo)
        historical = jj([*pinned, "log", "-G", "-r", "@", "-T", identity], repo)
        fact(f"{mode}-snapshot", commit_changed=original.splitlines()[0] != current.splitlines()[0],
             change_preserved=original.splitlines()[1] == current.splitlines()[1], historical_preserved=historical == original)
        if mode == "colocated":
            options = ["-c", "user.name=Probe", "-c", "user.email=probe@example.invalid", "-c", f"core.hooksPath={os.devnull}"]
            run(git_binary, [*options, "add", "file.txt"], repo)
            trace = base / "git-control.trace"
            run(git_binary, [*options, "commit", "-m", "git-control"], repo, {"GIT_TRACE2_EVENT": str(trace)})
            events = [json.loads(line) for line in trace.read_text().splitlines()]
            fact("git-control-trace", events=len(events), event_kinds=sorted({event["event"] for event in events}))
        second = base / f"{mode}-second"
        jj(["workspace", "add", "--name", "second", str(second)], repo)
        jj([*safe, "root"], second)
        jj([*safe, "workspace", "list", "-T", 'json(name) ++ "\\t" ++ json(root) ++ "\\n"'], second)
        jj(["edit", "default@"], second)
        jj([*safe, "workspace", "list", "-T", 'if(target.current_working_copy(), json(name) ++ "\\n")'], second)
        for path in (repo / ".jj/repo/store/git_target", second / ".jj/repo", repo / ".jj/repo/store/type"):
            fact("layout", path=clean(path), value=clean(path.read_text()))

    repo = base / "divergent"
    jj(["git", "init", "--no-colocate", str(repo)], base)
    operation = jj([*safe, "op", "log", "-G", "-n", "1", "-T", "id"], repo)
    for description in ("one", "two"):
        jj([f"--at-operation={operation}", "describe", "-m", description], repo)
    before = digest(repo)
    jj([*safe, "root"], repo)
    jj([*safe, "op", "log", "-G", "-n", "1", "-T", "id"], repo, expected=1)
    jj([*safe, "log", "-G", "-r", "@", "-T", "commit_id"], repo, expected=1)
    after = digest(repo)
    fact("divergent-safe", before=before, after=after, unchanged=before == after)
    assert before == after
    jj(["--ignore-working-copy", "op", "log", "-G", "-n", "1", "-T", "id"], repo)
    after_merge = digest(repo)
    fact("divergent-ignore-only", before=after, after=after_merge, changed=after != after_merge)
    assert after != after_merge

with args.output.open("x") as evidence:
    json.dump(records, evidence, indent=2)
    evidence.write("\n")
print(f"Wrote {len(records)} sanitized evidence records to {args.output}")
