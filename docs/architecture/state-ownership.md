# State Ownership

Every mutable state owner, its serialization model, and the definitive answer
to "what is ordered through the per-family actor flow". Verified 2026-07-21.

## The serialization verdict

**Family-serialized (ordered per repository family, via
`coordinator → family_actor (mpsc) → reducer → per-family exec-lock →
side-effect application`):**
- FamilyState (refs, worktrees, applied_seq, watermarks) — in-memory, actor-owned.
- Working logs (`.git/ai/working_logs/<base_commit>/`) — every mutation path
  (`repo_storage::append_checkpoint`, `write_all_checkpoints`,
  `mutate_all_checkpoints`, the post-rewrite filters in
  `git_op_side_effects.rs`) executes inside the family actor's side-effect
  pipeline.
- Post-commit note generation and rewrite note migration — invoked from
  `actor_coordinator_{side_effects,rewrites}.rs` under the family exec-lock.

**Mutex-guarded but NOT family-ordered (deliberate — these are queues/caches,
not attribution decisions):**
- The five SQLite stores (`model/repository/*`): process-global
  `OnceLock<Mutex<…>>` each; writers are the telemetry worker (tokio task),
  attribution-recovery backfill, stream worker, and notes backend queue.
  SQLite WAL + the Mutex make each store internally consistent; ordering
  across stores is not guaranteed and not required.
- Telemetry buffers and upload dispatch (`telemetry_worker.rs` statics).

**Single-process CLI paths (no daemon ordering, no concurrency within an
invocation):** config file writes (`git-ai config`), `notes migrate`,
`fetch-notes`, install/uninstall. These mutate user-config or perform
explicit migrations; they do not race the actor flow for the same fact.

Residual risk (accepted, documented): notes rows can be written by the family
flow (sqlite backend post-commit) and refreshed by cache
imports/`fetch-notes`; the `origin` column ('local' > 'cache'/'queue') plus
never-overwrite-local upserts resolve the priority (`notes_db.rs`).

## Singleton inventory

| Static | Location | Guard | Persistence / recovery |
|---|---|---|---|
| `CONFIG` | `config/mod.rs:82` | OnceLock | snapshot of config.json; daemon paths use `Config::fresh()` to observe edits |
| `AUTHOR_CONFIG_CACHE` | `config/mod.rs:107` | Mutex, 15s TTL + file fingerprint | recomputed on expiry |
| `DISTINCT_ID` | `config/mod.rs:596` | OnceLock | persisted id, read-once |
| `TEST_FEATURE_FLAGS_OVERRIDE` | `config/file.rs` | RwLock, cfg(test) | test-only |
| `METRICS_DB` | `model/repository/metrics_db/schema.rs:75` | OnceLock<Mutex> | SQLite v5; retry queue survives restarts |
| `NOTES_DB` | `model/repository/notes_db.rs:55` | OnceLock<Mutex> | SQLite v2; `origin` rows: local never evicted, cache evictable, queue retried |
| `INTERNAL_DB` | `model/repository/internal_db.rs:82` | OnceLock<Mutex> | legacy prompts/CAS queue (dormant) |
| `BASH_HISTORY_DB` | `model/repository/bash_history_db.rs:92` | OnceLock<Mutex<Result…>> | SQLite v2 |
| streams DB handle | `model/repository/streams_db.rs` | Arc<Mutex> per worker | SQLite v4 watermarks |
| `DAEMON_TELEMETRY_HANDLE`, `DAEMON_INTERNAL_TELEMETRY`, upload flags | `operations/daemon/telemetry_{handle,worker}.rs` | OnceLock/AtomicBool | in-memory; buffers flushed every 3s |
| `REFRESH_LOCK` (OAuth) | `clients/api/client.rs:15` | Lazy<Mutex> | serializes in-process token refresh; cross-process races accepted |
| `LAST_METRICS_UPLOAD_STARTED_AT` | `clients/api/metrics.rs:15` | OnceLock<Mutex> | 500ms upload rate limit, resets on restart |
| `DAEMON_PROCESS_ACTIVE` | `operations/daemon/daemon_config.rs:43` | AtomicBool | process-lifetime flag |
| Git alias cache | `operations/daemon/git_backend.rs:56` | per-family map, 60s stale-while-revalidate | re-resolved on expiry |

## Lock landscape

No site holds multiple lock kinds simultaneously: OAuth refresh (std Mutex),
config (OnceLock), each DB (std Mutex around its connection; SQLite
transactions nest inside), daemon coordination (tokio AsyncMutex for the
normalizer + per-family async exec-locks + std Mutex for the sequencer map).
File locks: `utils::LockFile` for installer/upgrade exclusivity only. Keep it
this way — new cross-lock interactions require documentation here first.
