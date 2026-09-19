# Mutating Git operation coverage

Fork snapshot: [`dd4d5b5c200d272b5343f87f1ef69a0d7a96b401`](https://github.com/woud420/git-ai/commit/dd4d5b5c200d272b5343f87f1ef69a0d7a96b401), inspected 2026-09-19 UTC. This matrix records representative registered tests and their limits; it is not an exhaustive support guarantee or a CI pass report. Open PRs below are not part of this baseline.

Git-command processing is asynchronous and Trace2-driven. A mutating Git command is not itself proof that an AI authored its content. Collection must be enabled for the repository; checkpoint content, usable historical attribution and exact ref evidence determine what can be attributed. Unknown lines remain untracked rather than becoming known-human by default.

## Registered integration evidence

These modules are registered in [`tests/integration/main.rs`](../../tests/integration/main.rs). Each row names a concrete non-ignored test, rather than counting an entire suite as coverage of every option. Read its assertions before broadening the stated scope.

| Git command | Tested partition | Registered test | Oracle | Remaining boundary |
|---|---|---|---|---|
| `commit` | Normal staged AI edits | [`simple_additions::test_simple_additions_with_base_commit`](../../tests/integration/simple_additions.rs) | Committed lines retain checkpoint attribution. | Empty, fixup and message-reuse variants need their own oracles. |
| `add / commit` | Partial staging | [`simple_additions::test_new_file_partial_staging_two_commits`](../../tests/integration/simple_additions.rs) | Only staged lines commit; remaining AI survives the next commit. | This does not prove add --pathspec-from-file or every index mutation. |
| `commit --amend` | Mixed staged/unstaged content | [`amend::test_amend_with_partially_staged_mixed_content`](../../tests/integration/amend.rs) | Amended attribution and residual working content stay separate. | New option combinations require explicit cases. |
| `reset --hard` | Discard pending working log | [`reset::test_reset_hard_deletes_working_log`](../../tests/integration/reset.rs) | Hard reset drops pending state. | Same-HEAD recreation safety is a separate boundary; see open work below. |
| `reset --soft / --mixed` | Reconstruct committed AI | [`reset::test_reset_soft_reconstructs_working_log`](../../tests/integration/reset.rs) | Unwound committed AI survives recommit. | Pending checkpoint / INITIAL precedence is a confirmed gap; this case does not cover it. |
| `reset --keep` | Forward move with pending AI | [`reset::test_forward_keep_reset_preserves_uncommitted_ai_attribution`](../../tests/integration/reset.rs) | Pending edits migrate to the forward base. | Do not generalize to every backward --keep/--merge dirty-index shape. |
| `reset pathspec` | Index-only reset | [`reset::test_reset_with_pathspec`](../../tests/integration/reset.rs) | Selected index changes preserve worktree attribution through commit. | Pathspec-file and arbitrary magic require dedicated cases. |
| `rebase` | Mixed line attribution | [`rebase::test_rebase_preserves_exact_mixed_line_attribution_in_single_file`](../../tests/integration/rebase.rs) | Rewritten commits retain AI/human line boundaries. | Abort/skip cleanup remains incomplete; see open work. |
| `rebase -i` | Reordered commits | [`rebase::test_rebase_interactive_reorder`](../../tests/integration/rebase.rs) | Reordered history retains attribution. | Do not infer coverage of every edit/drop/fixup/update-refs combination. |
| `merge` | Source contributions | [`merge_rebase::test_blame_after_merge_with_ai_contributions`](../../tests/integration/merge_rebase.rs) | Merged source AI remains visible in blame. | The complex-merge scenario in this suite is ignored and is not counted. |
| `merge conflict` | Explicit AI resolution | [`merge_rebase::test_merge_conflict_ai_resolution_outside_session`](../../tests/integration/merge_rebase.rs) | Checkpointed conflict resolution is attributed. | Continuation and discard controls have separate gaps. |
| `merge --squash` | Mixed source additions | [`squash_merge::test_prepare_working_log_squash_with_mixed_additions`](../../tests/integration/squash_merge.rs) | Squash preparation preserves mixed attribution. | Abandoned squash and cold/delayed source evidence need separate checks. |
| `cherry-pick` | Multiple source commits | [`cherry_pick::test_multiple_commits_cherry_pick`](../../tests/integration/cherry_pick.rs) | Source attribution reaches destination commits. | Abort/skip cleanup and mainline variants are not established by this case. |
| `cherry-pick -n` | Deferred commit | [`cherry_pick::test_cherry_pick_no_commit_defers_to_final_commit_tree`](../../tests/integration/cherry_pick.rs) | Attribution follows the final committed tree. | Multi-source/empty policy combinations need separate evidence. |
| `revert` | Explicit older source | [`rewrite_ops_attribution::test_revert_older_commit_restores_original_ai_attribution`](../../tests/integration/rewrite_ops_attribution.rs) | Restored lines recover the original AI source. | The multi-revert test asserts only the first restored source; later source recovery remains a gap. |
| `stash push / pop / apply` | Conflict restoration | [`stash_attribution::test_stash_pop_conflict_preserves_ai_attribution_without_new_checkpoint`](../../tests/integration/stash_attribution.rs) | Restored AI survives a conflict without a new checkpoint. | --keep-index, --staged, --patch, create/store and clear need distinct partition/lifecycle oracles. |
| `stash pathspec` | Leave unrelated paths live | [`stash_attribution::test_stash_push_pathspec_excludes_unstashed_file_from_stash_log`](../../tests/integration/stash_attribution.rs) | The stash log excludes unstashed paths. | This is not evidence for every Git pathspec form. |
| `checkout` | Branch migration | [`checkout_switch::test_checkout_branch_migrates_working_log`](../../tests/integration/checkout_switch.rs) | Pending working attribution moves with checkout. | Source-path restore and conflict stages have separate provenance boundaries. |
| `switch` | Discard changes | [`checkout_switch::test_switch_discard_changes_deletes_working_log`](../../tests/integration/checkout_switch.rs) | Explicit discard removes pending working attribution. | Orphan/tracking/failure modes require dedicated evidence. |
| `branch` | Delete and recreate | [`rewrite_ops_attribution::test_branch_delete_recreate_resets_trace2_ref_cursor`](../../tests/integration/rewrite_ops_attribution.rs) | Recreated branch does not reuse the prior cursor. | Rename/copy/upstream-only classification is not equivalent to attribution coverage. |
| `worktree` | Checkpoint routing | [`worktrees::checkpoint_routes_to_linked_worktree_when_cwd_is_main_repo`](../../tests/integration/worktrees.rs) | Checkpoint evidence is assigned to the linked worktree. | Remove/move/repair/prune lifecycle and recreation require additional cases. |
| `pull` | Rebase with autostash | [`pull_rebase_ff::test_pull_rebase_committed_and_autostash_preserves_all_authorship`](../../tests/integration/pull_rebase_ff.rs) | Committed and pending AI survive pull rebase/autostash. | Merge-pull and rejected fast-forward paths need separate evidence. |

## Other registered test targets

Cargo discovers these standalone targets under `tests/`; the core CI shard enumerates them in [the test workflow](../../.github/workflows/test.yml).

| Git command | Partition | Registered test | Oracle / limit |
|---|---|---|---|
| `commit-tree / update-ref` | Multi-commit rewrite and stdin HEAD update | [`test_multi_commit_plumbing_rewrite_single_update_ref`](../../tests/commit_tree_update_ref.rs) | Vendor-neutral replay maps source attribution through one final ref move. Also inspect test_update_ref_stdin_head_with_new_content_preserves_attribution. |
| `fetch` | Selected remote notes import | [`fetch_sync_imports_selected_remote_and_preserves_local_notes`](../../tests/fetch_notes_sync.rs) | Imports notes while preserving local notes; unsupported command shapes have explicit negative controls in the same target. |
| `clone` | Clone notes import | [`notes_sync_clone_fetches_authorship_notes_from_origin`](../../tests/notes_sync_regression.rs) | Notes arrive from origin; this does not establish every bare/shallow/no-checkout variant. |

## Gaps and weaker evidence

A Git command appearing in fixture setup, a parser test, or Git core compatibility tests does not establish a line-attribution oracle. The entries below intentionally make no end-to-end attribution claim.

| Command / partition | Current evidence boundary | Required oracle before claiming coverage |
|---|---|---|
| `push` | [Upstream-setting tests](../../tests/integration/push_upstream_authorship.rs) exercise notes publication. | Capture the actual destination and preserve checkpoint progress under blocked transport; see #304/#305 below. |
| `am`, `apply`, `fast-import`, `filter-branch` | No complete command-window attribution contract is established here. | Every actual destination, including partial success, must be attributed from immutable evidence; include an external/non-AI negative control. |
| `restore`, `clean`, `rm`, `mv` | The baseline command policy does not mark these as family-sequenced mutation roots. | Assert exact discard/rename behavior, same-content recreation, and unrelated pending attribution; see #301/#302. |
| `add`, `update-index`, `read-tree`, `write-tree`, `hash-object`, `mktree` | Staging/plumbing appears in existing fixtures; object or index mutation alone supplies no author. | Preserve pre-existing staged work, attribute only proven new content, and check eventual commit lines. |
| `tag`, `remote` | [Invocation classification](../../src/operations/git/command_classification.rs) separates some queries from mutations. | Ref/config mutation must leave unrelated notes and pending logs intact; classification alone is insufficient. |
| `notes`, `replace`, `symbolic-ref` | These are not all equivalent to ordinary commit/ref rewriting. | Identify which explicit transitions can preserve provenance and reject ambiguous mappings. |
| `config`, `credential`, `init` | Repository-administration behavior and test setup are not broad attribution coverage. | Cover selected repository context and absence of cross-repository corruption. |
| `gc`, `maintenance`, `pack-refs`, `prune`, `reflog expire` | Administrative classification is not cursor-retention evidence. | Preserve valid notes and fail closed when historical evidence has been removed or compacted. |
| `sparse-checkout`, `submodule` | No complete lifecycle guarantee is established by this matrix. | Hidden/revealed paths and nested repository families must preserve isolation and exact source attribution. |
| Conflict `--abort` / `--skip` | Characterization found discarded resolution reuse for merge/cherry-pick/revert and skip modes, plus unrelated pending attribution loss on rebase abort. | Recreate discarded bytes without a checkpoint and assert untracked attribution, while unrelated pending AI remains AI. |
| Backward soft/mixed reset with pending edits | The existing reconstruction tests do not protect newer checkpoint/INITIAL state. | Pending AI must survive history with no usable notes; pending human edits must override older AI history. |
| Multi-revert historical restoration | The existing multi-revert test checks content for all files but AI restoration only for the first. | Assert restored attribution for every source/destination without per-commit Git spawns or resurrecting superseded authorship. |

## Published work outside the baseline

Status here means the change was submitted for review; consult each PR for its exact head, validation and merge status. It does not upgrade the baseline rows automatically.

| PR | Scope | Still outside its claim |
|---|---|---|
| [#301](https://github.com/woud420/git-ai/pull/301), [#302](https://github.com/woud420/git-ai/pull/302) | Workspace mutation discard/mode coverage. | General live-index inference and all conflict/source restore variants. |
| [#306](https://github.com/woud420/git-ai/pull/306) | Pending AI and newer human INITIAL preservation across backward soft/mixed resets. | Historical multi-revert recovery and abort/skip cleanup. |
| [#303](https://github.com/woud420/git-ai/pull/303) | Exact merge/revert continuation and history-control characterization. | The abort/skip cleanup failures above. |
| [#304](https://github.com/woud420/git-ai/pull/304), [#305](https://github.com/woud420/git-ai/pull/305) | Captured push destinations and bounded asynchronous notes delivery. | Crash-persistent notes retry and arbitrary custom transport helpers. |
| [#297](https://github.com/woud420/git-ai/pull/297), [#299](https://github.com/woud420/git-ai/pull/299) | Separately submitted same-HEAD hard-reset discard and direct fast-forward merge carryover. | Other reset/merge partitions. |

## Fork boundaries and maintenance

- External stacking-tool compatibility is not a supported fork surface; retain generic `commit-tree` / `update-ref` behavior. See [the contributor workflow policy](../../tests/fork_workflow_policy.rs).
- The native jj observer is separate from Git rewrite processing. Its evidence does not imply Git operation support; see [the jj design](../decisions/2026-09-07-jj-support-design.md).
- [Git command policy](../../src/operations/git/command_policy.rs), invocation classification, semantic analysis, effect execution and regression coverage are different levels of evidence. A recognized command can still have unsupported variants.
- [Git core compatibility](../../tests/git-compat/run-core-tests.py) exercises Git behavior under the integration. It does not replace committed line-attribution assertions.
- `TestRepo` pins the Git Notes backend by default; production defaults to SQLite. These examples do not imply backend parity. Inspect [SQLite backend tests](../../tests/integration/sqlite_notes_backend.rs) separately.
- When adding a claim, name the registered test, inspect its exact line/metadata/state assertions, check `#[ignore]` and platform gates, and record delayed processing, failed/partial commands and negative controls where applicable. Update this snapshot after merging the corresponding work.

Use `make test CARGO_TEST_ARGS="--test integration" TEST_FILTER=reset::` for the reset integration cases, or `make test CARGO_TEST_ARGS="--test commit_tree_update_ref" TEST_FILTER=test_multi_commit_plumbing_rewrite_single_update_ref` for the named plumbing case. Append `EXTRA_TEST_BINARY_ARGS="--list"` to inspect registration without running tests. Match validation claims to the tested commit and platform; a source inventory alone is not a passing runtime result.
