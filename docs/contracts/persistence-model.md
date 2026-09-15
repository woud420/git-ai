# Persistence Model

Each fact below names its durable owner, derived copies and implementation.
A new writer or store must update this contract; process mutexes and family
sequencing do not imply cross-store atomicity. See
[state-ownership.md](../architecture/state-ownership.md) for execution ordering.

## Facts and authorities

| Fact | Durable owner / derived copies | Implementation and recovery |
|---|---|---|
| Authorship notes (sqlite backend, default) | `notes-db` local-primary rows; Git copy at `refs/notes/ai` (only if exported/migrated); existing legacy refs remain a read fallback that can populate cache rows | `operations/git/notes_api.rs`, `operations/git/notes_store.rs` (`SqliteNoteStore`); local rows are never cache-evicted. |
| Authorship notes (git_notes backend, opt-in) | Repository `refs/notes/ai`; this backend's reads/writes do not use notes-db | `operations/git/notes_store.rs` (`GitNotesStore`) delegates to `operations/git/refs.rs`. |
| Authorship notes (http backend, opt-in) | Remote service; notes-db queues pending writes and caches reads | `operations/git/notes_store.rs` (`HttpNoteStore`), `daemon/telemetry_worker/notes_flush.rs`; successful upload retains local rows. |
| Working checkpoints | Repository storage `working_logs/<base_commit>/`, checkpoint index journal and referenced content/initial-attribution files | `operations/git/repo_storage.rs`, `operations/git/repo_storage/checkpoint_journal/`; bounded decoded cache is derived and revision-checked. Rewrite/post-commit paths migrate or archive logs. |
| Pending checkpoint delivery | Versioned ready records in selected outbox root; applied delivery ID retained in a working-log checkpoint | `model/repository/checkpoint_outbox/`, `daemon/checkpoint_outbox_worker.rs`, `daemon/checkpoint.rs` (`execute_resolved_checkpoint`); replay re-enters family sequencing, checks recorded IDs/durability, and quarantines repeated failures. |
| Ref/worktree facts | Git refs/reflogs; daemon `FamilyState` and `RefCursor` are process-local projections | `daemon/ref_cursor/enrichment.rs`, `daemon/family_actor.rs`; asynchronous ingress offsets are hints, not universally exact start snapshots. |
| Transcript positions/session records | Streams DB | `model/repository/streams_db.rs`, `daemon/stream_worker.rs`; stream-specific watermark strategies track consumed input. |
| Transcript source/content | Agent transcript files are source inputs; redacted session events become derived metrics rows | `operations/streams/`, `daemon/stream_worker.rs`; checkpoint stream paths are validated in `daemon/checkpoint_stream_authority.rs`. |
| Metrics events/retry state | Metrics DB | `model/repository/metrics_db/`; retained history, age pruning and bounded upload retry queue. |
| Bash tool-use provenance | Bash-history DB | `model/repository/bash_history_db.rs`; control writers in `daemon/actor_coordinator_control_requests.rs`. |
| Legacy prompts/CAS queue | Internal DB | `model/repository/internal_db.rs`, `daemon/telemetry_worker/cas_flush.rs`; retained compatibility store. |
| Native jj evidence/registration/admission | Explicitly selected jj journal; immutable receipts and staged/current native records | `model/repository/jj_observation_journal/`, `operations/jj/`; evidence capture is separate from Git attribution. |
| jj observer intent | Daemon-owned observer-intent DB; runtime worker/job state is derived | `model/repository/jj_observer_intent/`, `daemon/jj_observer/`; persisted target/revision/enabled/blocked state. |
| User config and credentials | `~/.git-ai/config.json`, auth storage selected by configuration, and environment overrides | `config/`, `clients/auth/`; `CONFIG` is a per-process snapshot, while `Config::fresh()` reloads. API keys are persisted config fields, so not all serialization is redacted. |

Source paths beginning `model/`, `operations/`, `config/` or `clients/` are
relative to `src/`; `daemon/` abbreviates `src/operations/daemon/`.

## SQLite owners

Default internal paths may be overridden by the corresponding test/runtime
configuration. Exact resolution belongs to each owner's constructor.

| Store | Default path / schema | Access and writers |
|---|---|---|
| Notes | `~/.git-ai/internal/notes-db`, v2 | `NotesDatabase::global()` mutex; notes-store local/queue writes, cache imports and notes flush. |
| Metrics | `~/.git-ai/internal/metrics-db`, v5 | `MetricsDatabase::global()` mutex; event writers, telemetry/recovery and reingestion. |
| Internal | `~/.git-ai/internal/db`, v3 | `InternalDatabase::global()` mutex; compatibility prompt/CAS paths. |
| Bash history | `~/.git-ai/internal/bash-history-db`, v2 | `BashHistoryDatabase::global()` mutex; Bash control recording. |
| Streams | `~/.git-ai/internal/transcripts-db`, v4 | Injected `StreamsDatabase`, `Arc<Mutex<Connection>>`; stream worker. |
| jj observation journal | Caller-selected path, v4 | Explicit `JjObservationJournal` connection; schema/registration/admission transactions. |
| jj observer intent | Observer-selected daemon path, v1 | Per-operation connections; exact-schema verification and revision-checked transactions in `model/repository/jj_observer_intent/mod.rs` (`{load,replace}`). |

## Notes write/fallback contract

`operations/git/notes_api.rs` composes backend selection and fallback; its
`export_notes_to_git_refs` is called by `operations/commands/notes_migrate.rs`
when exporting SQLite local-primary notes to Git Notes.
`operations/git/notes_store.rs` provides concrete backend primitives. Existing asymmetries are
intentional compatibility behavior:

- **SQLite:** DB-first reads, then legacy refs fallback with best-effort cache
  backfill. `read_notes_batch` propagates refs errors; `read_note` and
  `read_authorship` use `Option` and suppress read errors.
- **HTTP:** writes queue locally and call `daemon/telemetry_handle.rs` (`submit_notes`) to
  wake uploads. Batch misses can fetch remotely before refs fallback. A refs
  fallback does not backfill the HTTP cache. This is an explicit store-to-daemon
  notification dependency.
- **Git notes:** `GitNotesStore` reads/writes only `refs/notes/ai` via the Git
  primitives. `refs/notes/ai-display` is disposable display materialization
  (`operations/git/notes_api.rs`, `materialize_notes_for_display`), not another authority.

`model/repository/notes_db.rs` (`upsert_local_notes_batch`) replaces local-primary content.
`cache_synced_notes` skips existing local rows. HTTP queue `UPSERT_NOTE_SQL`
updates content on conflict; unchanged content preserves synced/retry state,
changed content resets it. The cache predicate is not a universal priority rule
for every writer. Backend migrations use `operations/commands/notes_migrate.rs`; independent CLI
and daemon processes have no shared family-order guarantee.

Notes and metrics retry queues allow up to six failed attempts before stopping
automatic upload
(`model/repository/notes_db.rs` (`dequeue_pending`), `model/repository/metrics_db/upload_queue.rs`); metadata
and retry times persist; terminal metric errors can stop retries earlier. Outbox application retries use process-local counters,
poll backoff and quarantine after five failures (`daemon/checkpoint_outbox_worker.rs`).
Daemon diagnostic logs have no durable retry queue and are best-effort after
dispatch (`daemon/telemetry_worker/daemon_log_upload.rs`). These guarantees do not make
arbitrary command effects or repeated content checkpoints exactly-once.
