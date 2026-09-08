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

## Implementation verification: pending

P1 is being implemented. Implementation tests, regression checks, review, and integration results are pending and must be recorded separately by the coordinating task. The successful runtime probe does not verify Git AI attribution, checkpoint routing, asynchronous reconciliation, or native jj rewrite support.
