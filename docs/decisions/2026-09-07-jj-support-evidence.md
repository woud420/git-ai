# Native jj support: runtime evidence and delivery status

Date: 2026-09-07. Scope: discovery experiments for the proposed native jj integration. These results do not establish implemented attribution support or a supported-version range.

Proposal: [Native jj support — proposal, architecture, and delivery plan](https://linear.app/polarcoordinates/document/native-jj-support-proposal-architecture-and-delivery-plan-f4fd7edf1f13).

## Runtime and provenance

The probes ran on macOS arm64 using the official [jj v0.45.1 release](https://github.com/jj-vcs/jj/releases/tag/v0.45.1), downloaded into a temporary tools directory. No system installation or existing repository was changed. Test repositories used isolated Git and jj configuration and synthetic author details.

| Item | Verified value |
| --- | --- |
| Binary version | `jj 0.45.1-7c41cdeb16b6b321c64e789a966b6adf723816a5` |
| Release asset | `jj-v0.45.1-aarch64-apple-darwin.tar.gz` |
| Archive SHA256 | `51ba42e3d0682616f6eb015045bfe45289b396f03511f9897f645ce8e9272743` |
| Binary SHA256 | `6036ef29214876d932ace0e4c66cd5af5089edd3204ed004d51d0bb99ba759cf` |

The archive digest matched the GitHub release asset metadata before extraction. The release binary's bundled command help and template documentation were also inspected.

## Reproduction

The [probe script](../../scripts/benchmarks/jj/probe_runtime.py) uses standard Python, an explicit jj binary, and an explicit real Git binary. It creates temporary repositories, runs bounded local experiments, removes those repositories afterward, and writes sanitized JSON evidence. It does not download binaries or change global configuration. The evidence output must be a new file.

```sh
python3 scripts/benchmarks/jj/probe_runtime.py \
  --jj <absolute-jj-binary> \
  --git <absolute-real-git-binary> \
  --output <new-evidence-json-path>
```

The script was executed successfully on the runtime above and produced 57 records. The task-local result is `/private/tmp/git-ai-jj-evidence-20260907.json`; it is local evidence, not a committed artifact. Binary files and the original exploratory temporary repositories are also local only.

The commands below show the tested program arguments; `<jj>`, `<repo>`, `<op-id>`, and `<trace-file>` are placeholders. The script supplies isolated environment values and passes arguments directly without shell interpolation.

```sh
<jj> --no-pager --color=never --ignore-working-copy --at-operation=@ root
<jj> --no-pager --color=never --ignore-working-copy --at-operation=@ \
  op log -G -n 1 -T id
<jj> --no-pager --color=never --ignore-working-copy --at-operation=<op-id> \
  log -G -r @ -T \
  'commit_id ++ "\n" ++ change_id ++ "\n" ++ json(parents.map(|p| p.commit_id()))'
<jj> --no-pager --color=never --ignore-working-copy --at-operation=<op-id> \
  workspace list -T 'json(name) ++ "\t" ++ json(root) ++ "\n"'
```

Run `root` from a nested directory to verify workspace discovery. The operation query returns a full 128-character hexadecimal ID; subsequent reads use that immutable ID. The working-copy query returns full commit and change IDs and a JSON array of parent commit IDs.

## Observed behavior

| Experiment | Result on jj v0.45.1 |
| --- | --- |
| Colocated and standalone repositories with unsnapshotted file edits | Discovery commands above preserved a digest of every repository file path and its contents. This did not check all filesystem metadata or every possible repository state. |
| Run ordinary `jj status` after the edits | The working-copy commit ID changed; its change ID stayed the same. A subsequent query pinned to the earlier operation returned the original commit. |
| `GIT_TRACE2_EVENT=<trace-file> <jj> commit -m first` | No trace file appeared in either colocated or standalone repositories. |
| Same Trace2 environment with a real Git control commit | A trace file appeared with 45 events, including `start`, `cmd_name`, and `exit`. The exact event count is runtime-specific. |
| Initial workspace layout | `.jj/repo` was a directory. Its `store/type` contained `git`. |
| Secondary workspace layout | `.jj/repo` was a text pointer such as `../../standalone/.jj/repo`, resolved relative to the workspace's `.jj` directory. |
| Backing Git directory | `store/git_target` contained `../../../.git` for colocation or `git` for standalone storage, resolved relative to `store`. These are version-specific observations, not a stable public layout contract. |

Local jj commits therefore cannot be assumed to pass through Git's Trace2 observer. This experiment does not prove that all jj commands emit no Git events; network operations or other subprocesses may behave differently.

### Divergent and redundant operation heads

The probe created two genuinely divergent operations by running `jj --at-operation=<same-base-op> describe -m one` and then `describe -m two`.

- With `--ignore-working-copy --at-operation=@`, both the operation query and working-copy query exited 1 because `@` resolved to more than one operation. The repository content digest stayed unchanged.
- The `root` command still succeeded. Finding a root does not prove there is an unambiguous operation to observe.
- With only `--ignore-working-copy`, `op log` reported automatic concurrent-operation resolution and changed the repository content digest. This flag alone is insufficient for an observer.

**Source-derived caveat:** even `--at-operation=@` is not a strict zero-write guarantee. In v0.45.1 the [`@` operation resolver](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/op_walk.rs#L90) calls [`resolve_op_heads`](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/op_heads_store.rs#L104), which can lock the head store and remove redundant ancestor head files. This normalization case is distinct from the genuinely divergent-head case exercised above. An adapter that requires strictly read-only observation needs to account for it; immutable operation IDs are the preferred input after resolution.

### Workspace identity trap

`log -r @ -T 'json(working_copies.map(|w| w.name()))'` returned `[]` when the repository had a single workspace. In a repository with two workspaces, running `jj edit default@` from the second workspace succeeded and made both workspaces point to the same commit.

After that, this query returned both `"default"` and `"second"`:

```sh
<jj> --no-pager --color=never --ignore-working-copy --at-operation=@ \
  workspace list -T 'if(target.current_working_copy(), json(name) ++ "\n")'
```

Neither working-copy commit identity nor `target.current_working_copy()` uniquely identifies the current workspace. Match its canonical root against explicit workspace-root output, and reject missing or ambiguous mappings. The bundled `WorkspaceRef.root()` documentation says roots can be unavailable for workspaces created before jj 0.38.0 or after directory moves/deletion. The last operation's originating workspace is also not a substitute for the current workspace.

## Delivery status

Snapshot supplied by the coordinating implementation task on 2026-09-07. Research evidence above is separate from implementation completion.

| Slice | Linear issue | Status |
| --- | --- | --- |
| Parent | [ENG-412](https://linear.app/polarcoordinates/issue/ENG-412) | In Progress |
| P0 | [ENG-413](https://linear.app/polarcoordinates/issue/ENG-413) | Backlog |
| P1 | [ENG-414](https://linear.app/polarcoordinates/issue/ENG-414) | In Progress |
| P2 | [ENG-415](https://linear.app/polarcoordinates/issue/ENG-415) | Backlog |
| P3 | [ENG-416](https://linear.app/polarcoordinates/issue/ENG-416) | Backlog |
| P4 | [ENG-417](https://linear.app/polarcoordinates/issue/ENG-417) | Backlog |
| P5 | [ENG-418](https://linear.app/polarcoordinates/issue/ENG-418) | Backlog |
| P6 | [ENG-419](https://linear.app/polarcoordinates/issue/ENG-419) | Backlog |
| P7 | [ENG-420](https://linear.app/polarcoordinates/issue/ENG-420) | Backlog |
| P8 | [ENG-421](https://linear.app/polarcoordinates/issue/ENG-421) | Backlog |
| P9 | [ENG-422](https://linear.app/polarcoordinates/issue/ENG-422) | Backlog |
| P10 | [ENG-423](https://linear.app/polarcoordinates/issue/ENG-423) | Backlog |

## Implementation verification

P1 is implemented locally on `codex/jj-support-foundation` in the persistent
isolated worktree `/Users/jm/workspace/projects/git-ai-jj-support`.
The system installation is unchanged and the branch has not been pushed.
ENG-414 remains In Progress pending publication and human review; native jj
attribution remains unimplemented.

`git-ai debug context --json` reports schema version 1, `discovery_only`, VCS,
canonical workspace root, Git directory/common directory, jj repository/store
paths, and colocation. It resolves the nearest selected boundary without
subprocesses, daemon startup or attribution storage initialization. It supports
actual colocated/noncolocated jj, additional workspaces, Git worktrees, and
colocation through a Git file pointing at a separate Git directory.

Pointer/type reads are capped at 16 KiB. Unknown backends, invalid UTF-8 paths,
malformed pointers and dangling boundaries return explicit JSON errors.
jj pointers preserve raw path whitespace; Git `commondir` is authoritative.
This is paths-only discovery: it does not parse checkout protobufs, establish
jj's internal workspace ID, validate an operation frontier, or enable checkpoints.

The existing Git discovery and ingestion paths are unchanged. The shared
`is_valid_git_dir` predicate is reused; the diagnostic resolves selected-root
pointers itself because the existing helper walks ancestors and guesses common
directories from a parent named `worktrees`. A small help extraction reduced
`debug.rs` from 1642 to 1635 lines; the new resolver is 259 lines.

### Verified checks

All logs below are local files under `/private/tmp/git-ai-jj-support-`; the
suffix column completes each filename with `.log`.

| Check | Observed result | Log suffix |
| --- | --- | --- |
| TDD before production implementation | 11 behavioral failures | `red-expanded` |
| Added review regressions before fixes | 13 passed, 2 failed as predicted | `review-red` |
| Final context qualification, including real jj | 15 passed, zero ignored | `final-green` |
| Existing debug unit tests | 18 passed | `debug-regression` |
| Existing explicit-path Git checkpoint regression | 1 passed | `checkpoint-regression-unsandboxed` |
| Source file-length ratchet | 1 passed | `file-length` |
| Layer import policy | 8 passed | `layer-policy` |
| CI-equivalent Clippy 1.93 with warnings denied | Passed | `lint-msrv` |
| Formatting | Passed | `format-check` |
| Explicit jj lane with its binary omitted | 3 failures with the required-binary explanation, as intended | `jj-missing-binary` |

The three real-jj cases cover colocated dirty-tree nonmutation, standalone/additional
workspace pointers, and a colocated separate Git directory under a parent literally
named `worktrees`. The last case checks the external store's content manifest too.
Review also added a regression preventing a dangling nested marker from returning
an inner workspace root paired with an outer Git store.

A fresh native build preceded final GREEN (`review-build.log`).
The tested binary SHA256 is
`a0e1b4bcc400e41ca33f10aaadf652547a34531668bef48021bae2913864cb18`.
After verification, the worktree was moved from its temporary location to the
persistent path above; source contents and the tested binary were preserved.

### Reproducing the implementation tests

From this branch's worktree on macOS with GNU Make and an explicitly installed
test jj binary:

```sh
rtk gmake build
rtk proxy env \
  GIT_AI_TEST_BINARY_PATH="$PWD/target/debug/git-ai" \
  GIT_AI_TEST_JJ_BINARY=/absolute/path/to/jj \
  gmake test CARGO_TEST_ARGS="--test integration" \
  TEST_FILTER=debug_context EXTRA_TEST_BINARY_ARGS=--include-ignored TEST_THREADS=2
rtk gmake format-check
```

The existing test-binary override avoided a stalled nested Cargo build in this
host's TestRepo harness. Always rebuild after source edits before using that
override. It was used only by the test harness; the system git-ai installation
was not replaced.

CI lint uses `toolchain: msrv`; `Cargo.toml` currently specifies Rust 1.93.
The installed 1.93 toolchain's bin directory was placed first on PATH, with
`RUSTUP_TOOLCHAIN=1.93.0` and a separate target directory, then `gmake lint` passed.
Merely selecting rustup while Homebrew's `cargo-clippy` remains first on PATH
does not establish that toolchain. Host Clippy 1.98 reported
`chunks_exact_to_as_chunks` in unchanged `src/cli/hook_input.rs`; that file was
verified byte-identical to the fork baseline and was not edited.

The first checkpoint regression attempt could not bind its isolated test sockets
inside the sandbox. The same test passed when socket binding was permitted.
The full integration suite and Linux/Windows execution were not run for this
diagnostic slice; broad native-jj qualification remains ENG-423.

Next: ENG-413 must prove the bounded operation/lineage reader before ENG-415 can
start native event ingestion. Preliminary research and this diagnostic do not
satisfy that proof gate.
