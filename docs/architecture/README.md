# Architecture

git-ai is a single binary dispatching on `argv[0]` (`src/main.rs`, `src/cli/`):
invoked as `git` it transparently proxies the real Git binary; invoked as
`git-ai` it serves direct subcommands. Git-command attribution processing is
trace2-driven and asynchronous. The proxy does not infer attribution through
synchronous wrapper hooks; ingestion remains latency-sensitive.

## Dependency boundaries

The current boundary is narrower than a directory-wide layered graph:

- Model values and attribution computation sit below adapters and orchestration.
  `model/repository/` owns persistence. The model also contains ambient timestamp
  and ID constructors, supplied-stream readers and diagnostic instrumentation;
  it is not uniformly deterministic. See [inventory.md](inventory.md).
- The reducer and built-in analyzers operate on supplied values. They use model
  types, shared error representations, and the explicitly guarded pure Git
  grammar/policy kernels. Git OID syntax belongs to `model/git_oid.rs`;
  `operations::git::oid` retains the adapter-facing re-export.
- Git, persistence and network adapters perform effects; daemon/command/CLI
  code orchestrates them. Some existing modules mix these responsibilities,
  including the notes-store upload notification into the daemon. The whole
  directory graph is not claimed to be acyclic.

`tests/integration/layer_import_policy.rs` checks the concrete import directions
listed in the inventory. The [layer boundary audit](layer-boundary-audit.md)
records the evidence, Git-domain classification and measured decomposition.
The [P9 plan](../decisions/2026-07-20-layered-architecture-plan.md) is historical.

## The command pipeline

Unqualified source paths in the pipeline table are under `src/operations/daemon/`.

| Stage | Implementation and boundary |
|---|---|
| Observe | `socket_listeners.rs` receives trace2 frames. `actor_coordinator_ingest.rs` captures root metadata and asynchronous reflog offset hints when repository context becomes available. |
| Normalize | `trace_normalizer/` produces `model::domain::NormalizedCommand`; `GitBackend` resolves Git family/alias information. The envelope retains Git argv, source OIDs and offset hints. |
| Order and lock | `actor_coordinator_seq.rs` maintains ordered family entries. `actor_coordinator_drain.rs` (`drain_ready_family_sequencer_entries`) takes the family exec-lock before routing a ready command and retains it through effects. |
| Enrich | `coordinator.rs` routes to `family_actor.rs`; `RefCursor::enrich_command` in `ref_cursor/enrichment.rs` derives ref transitions from cursor-bounded reflog matching and supported immutable arguments. Ingress offsets are soft hints when an established cursor exists; cold seeds can be clamped to the command's own entry. Missing evidence is not replaced by a live-HEAD guess. |
| Classify and reduce | `family_actor.rs` supplies the pre-command ref snapshot and canonical worktree path to `reducer.rs` (`reduce_family_command_with_ref_snapshot`). Built-in analyzers classify the command; the reducer updates in-memory `FamilyState` and returns `AppliedCommand`. |
| Execute | `actor_coordinator_drain.rs` invokes `actor_coordinator_side_effects.rs`, `actor_coordinator_rewrites.rs` and the concrete commit/rewrite/working-log/notes effects. Successful exits and explicitly handled conflict/partial-success cases can produce effects. Failed rebase/cherry-pick commands also preserve continuation state. |

The family exec-lock surrounds actor reduction **and** effect execution. The
actor owns `FamilyState` and its `RefCursor`; the outer coordinator owns queue,
fence and effect state. `AppliedCommand` acknowledges reduction, not successful
effect completion. `actor_coordinator_control.rs` (`sync_family`) waits for ingress
and effects; `status_for_family` reports current sequence/error state without
that completion fence. Global commands use `global_actor.rs`.

Checkpoint control requests (`CheckpointRun` / `CheckpointDeliver`) enter through
`actor_coordinator_query.rs`. They join the same family sequencer; the drain
advances the actor sequence and runs `side_effect_helpers.rs` (`apply_checkpoint_side_effect`).
Outbox replay re-enters the delivery path. Transcript enrichment is separately
authorized by `checkpoint_stream_authority.rs` and consumed by `stream_worker.rs`.

## Authority, state and recovery

- **Observed facts:** trace2 frames, captured checkpoint requests/content and
  authorized agent transcript files. Reflog hints are asynchronously captured
  observations; cursor matching establishes usable historical ref transitions
  (`ref_cursor/enrichment.rs`, `initialize_from_command_reflog_start_offsets`).
- **Interpretation:** primary command/event classification is in `analyzers/`.
  Ref selection, reducer branch-state inference and rewrite execution retain
  operation-specific decisions. `SemanticEvent` is not a complete effect plan.
- **Attribution calculation:** `model/attribution_tracker/` computes attribution;
  `operations/authorship/` combines it with Git/notes/working-log IO. The tracker
  also emits timing diagnostics. See the inventory for the precise purity scope.
- **Durable authority:** notes depend on the selected backend; working logs hold
  checkpoint history; SQLite stores hold metrics/transcript positions and other
  facts listed in [persistence-model.md](../contracts/persistence-model.md).
- **Derived state:** family refs/worktrees are an in-memory observation cache,
  initialized empty. Their inferred branch/detached fields are not exact live
  Git authority. Watermarks and applied sequence are process-local; alias and
  decoded checkpoint-journal caches have separate invalidation rules.
- **Ordering:** daemon checkpoint and rewrite effects are serialized per queued
  repository family. CLI migrations/cache imports, upload workers and jj evidence
  capture have separate ownership. Source-family effects can synchronize notes
  to another repository; the family lock is not a universal destination lock.
  See [state-ownership.md](state-ownership.md).
- **Retries:** notes and metrics persist bounded upload retry state
  (`model/repository/{notes_db.rs,metrics_db/upload_queue.rs}`). Checkpoint outbox
  deliveries replay with poll backoff and quarantine after repeated failures
  (`checkpoint_outbox_worker.rs`). Diagnostic log uploads are best-effort,
  fire-and-forget (`telemetry_worker/daemon_log_upload.rs`).
- **Replay limits:** command effects run while draining in-memory entries;
  dequeue does not provide durable replay or exactly-once execution. Checkpoint delivery IDs recorded in
  the working log suppress reapplication and require durable-record validation
  (`checkpoint.rs`, `execute_resolved_checkpoint`). Identical file content alone
  is not a universal idempotency key. Cache imports preserve local notes;
  queue writes and local writes follow different upsert rules (`model/repository/notes_db.rs`).

## Compatibility and implementation references

Public boundaries are the authorship serialization format
(`specs/git_ai_standard_v3.0.0.md`, `src/model/authorship_log_serialization.rs`),
[CLI output](../contracts/cli-output.md),
[checkpoint interface](../contracts/checkpoint-interface.md) and
[notes HTTP contract](../contracts/notes-backend-spec.md). Internal Rust module
visibility alone is not a compatibility guarantee. `NormalizedCommand` is not
a durable replay format: its `trace_derived` field is skipped by serde.

Git edge handling lives in `ref_cursor/` and `operations/authorship/rewrite*`,
with daemon conflict/continuation helpers. OS transport lives in
`socket_listeners.rs`; installation/platform handling lives in the command
installers. The native jj observer captures evidence through its own journal;
it does not apply that evidence through the Git reducer.

- [daemon-trace2-ingestion-spec.md](daemon-trace2-ingestion-spec.md)
- [rewrite-ops-spec.md](rewrite-ops-spec.md)
- [state-ownership.md](state-ownership.md)
- [inventory.md](inventory.md)

- [Mutating Git operation coverage and remaining gaps](mutating-git-coverage.md)
