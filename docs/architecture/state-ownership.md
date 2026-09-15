# State Ownership

Principal mutable owners and synchronization boundaries, checked 2026-09-15.
The [layer boundary audit](layer-boundary-audit.md) lists the measured coordinator
slices. Each `actor_coordinator_*` file extends the same `ActorDaemonCoordinator`
in `src/operations/daemon/actor_types.rs`; a file split is not a state owner.

## What the family flow orders

`actor_coordinator_seq.rs` maintains ordered family entries.
`actor_coordinator_drain.rs` (`drain_ready_family_sequencer_entries`) obtains the
family exec-lock before dequeue/routing and holds it through actor reduction
and effects:

```
family sequencer → family exec-lock → coordinator → family actor
                 (held)                         → enrich + reduce
                                               → effect execution
```

The family actor owns `FamilyState` and `RefCursor`
(`family_actor.rs`, `spawn_family_actor`). Refs, worktrees, sequence and watermarks
start empty/zero. Watermark updates run through its mailbox. Ref/worktree maps
are observation caches, not authoritative historical snapshots; reducer branch
pairing can infer detached state when the branch is ambiguous.

Within this daemon flow, the lock orders checkpoint application, post-commit
note generation and rewrite/working-log migration. Concrete paths are
`actor_coordinator_drain.rs` → `side_effect_helpers.rs` (`apply_checkpoint_side_effect`)
→ `checkpoint.rs` (`execute_resolved_checkpoint`), and
`actor_coordinator_side_effects.rs` → commit/rewrite effect modules.
`AppliedCommand` and `applied_seq` record reduction before effects finish.
`actor_coordinator_control.rs` (`sync_family`) fences ingress and drains effects;
`status_for_family` alone is not a completion fence. Failed command effects are
reported, not automatically persisted for replay as an effect transaction.

The lock belongs to the queued command's family. Notes synchronization may touch
a destination repository, which is why `sync_family` retains a global effects
completion check. Storage APIs themselves do not require a family lock: for
example, `commands/status.rs` calls `repo_storage.rs` (`working_log_for_base_commit`),
which can create the log directory. The claim is about daemon attribution
mutation paths, not every filesystem mutation beneath `.git/ai`.

## Independent owners

| Owner | Guard / writers | Authority and recovery |
|---|---|---|
| Coordinator ingress, root slots, sequencers and execution locks (`actor_types.rs`) | Async normalizer mutex, short-lived map mutexes, atomics, mailbox and per-family exec-lock; ingestion, sequencing and drain methods | Process-local queue/fence state. The drain removes ready entries before applying them; restart does not replay a durable command queue. |
| Coordinator pending rebase/cherry-pick/squash and AI-edit maps | Coordinator mutexes; command/checkpoint effects | Continuation/filtering state; fields and methods are in `actor_types.rs` and `actor_coordinator_{worktree,side_effects,base}.rs`. |
| Notes (`NOTES_DB` in `model/repository/notes_db.rs`), metrics, internal and Bash-history SQLite handles | Process-global `OnceLock` mutex handles in `model/repository/{notes_db.rs,metrics_db/schema.rs,internal_db.rs,bash_history_db.rs}`; backend, telemetry/recovery and Bash-control writers | Per-store transactions/WAL; a Rust mutex coordinates one process, not all processes. Stores have no common transaction or family ordering. |
| Streams DB | `StreamsDatabase` owns `Arc<Mutex<Connection>>`; injected into `stream_worker.rs` | Persistent stream positions/session records; no global DB singleton (`model/repository/streams_db.rs`). |
| Native jj journal | Explicitly opened `JjObservationJournal` connection; native admission/registration workflows | Durable evidence, registrations and receipts (`model/repository/jj_observation_journal/`). No Git attribution application. |
| jj observer runtime/intent | `jj_observer::Observer` has a state mutex, mutation async mutex and bounded job slot; intent storage opens its own connections | `model/repository/jj_observer_intent/` persists selected target, enabled/blocked state and revision; observer runtime is separate from Git family sequencing. |
| Checkpoint outbox | Publishers and `checkpoint_outbox_worker.rs`; filesystem publication/consume protections in `model/repository/checkpoint_outbox/publication/` | Durable delivery records. Worker retries with in-memory counters and poll backoff, then quarantines; successful replay removes a record. Working-log delivery IDs suppress recorded reapplication. |
| Telemetry buffers/upload dispatch | Worker buffer async mutex plus upload flags; `telemetry_worker/`, `telemetry_handle.rs` | Notes/metrics use durable queues. Daemon logs remain best-effort memory buffers; failed dispatched uploads are dropped. |
| Config/migrations/installers | Separate CLI processes; config writes, `notes migrate`, `fetch-notes`, install/uninstall | No family-order guarantee or cross-process exclusion from concurrent daemon effects. Backend/SQLite/file behavior supplies the applicable local protections. |

Notes cache refreshes specifically cannot overwrite `origin='local'` rows:
`notes_db.rs` (`cache_synced_notes`) has that SQL predicate. This is not a universal
“local beats queue” rule: `UPSERT_NOTE_SQL` used for HTTP queue writes updates
content on conflict. `upsert_local_notes_batch` writes local-primary content.
Backend migration and concurrent cache/queue operations must be reasoned about
using these distinct methods; see [persistence-model.md](../contracts/persistence-model.md).

## Process caches and handles

| State | Implementation / guard | Refresh or lifetime |
|---|---|---|
| `CONFIG`, `DISTINCT_ID` | `config/mod.rs`, `OnceLock` | Config snapshot and persisted identity. Callers using `Config::fresh()` observe edits; other callers retain the snapshot. |
| `AUTHOR_CONFIG_CACHE` | `config/mod.rs`, `OnceLock<Mutex>` | 15-second TTL plus file fingerprint. |
| `TEST_FEATURE_FLAGS_OVERRIDE` | `config/file.rs`, test-only `RwLock` | Test lifetime. |
| `DAEMON_TELEMETRY_HANDLE`, `DAEMON_INTERNAL_TELEMETRY`, upload flags/run ID | `operations/daemon/telemetry_handle.rs`, `telemetry_worker/` | Process lifetime; worker loop schedules flushes. |
| OAuth `REFRESH_LOCK` | `clients/api/client.rs`, `LazyLock<Mutex>` | In-process token refresh serialization only. |
| `LAST_METRICS_UPLOAD_STARTED_AT` | `clients/api/metrics.rs`, `OnceLock<Mutex>` | In-process upload rate limiting. |
| `DAEMON_PROCESS_ACTIVE` | `operations/daemon/daemon_config.rs`, `AtomicBool` | Process lifetime. |
| `SystemGitBackend.alias_cache` | `operations/daemon/git_backend.rs`, shared mutex map keyed by family | 60-second stale-while-revalidate TTL. |
| Decoded checkpoint journal cache | `operations/git/repo_storage/checkpoint_journal/cache.rs`, `OnceLock<Mutex>` | Two entries / 800 KiB conservative retained-capacity estimate; exact SHA-256 plus byte length validates file revision. |

## Journal lock boundary

`model::repository::lock_file::LockFile` protects daemon ownership and
working-log journal publication. The journal owns storage paths, legacy-format
provenance and its bounded decoded-state cache. Cache checkout takes the journal
file lock before the short-lived cache mutex; publication holds only the file
lock, and lease return drops the file lock before taking the cache mutex.

Warm checkout hashes exact file bytes; cold decoding hashes the accepted byte
stream and compares it with the final file. Cache-lease publication and durable
success recheck SHA-256 plus length. Legacy uncached rewrite helpers retain file
locking and atomic replacement, without revision-CAS protection. These are
storage protections, not proof of one transaction across all attribution facts.
The cache capacity estimate includes retained capacities/allocator overhead and
alignment, not exact resident memory. Its independent entry limit is a second
bound. New cross-lock interactions must be documented here.
