# Persistence Model

One documented source of truth per persisted fact. There must never be two
undocumented authorities for the same fact; if a change introduces a second
writer or store for anything below, update this table in the same PR.

| Fact | Authoritative store | Caches / derived copies | Retention |
|---|---|---|---|
| Authorship notes (sqlite backend, default) | `~/.git-ai/internal/notes-db`, rows `origin='local'` | `refs/notes/ai` (only if exported/migrated); read-fallback backfills as `origin='cache'` | local rows never evicted |
| Authorship notes (git_notes backend, opt-in) | `refs/notes/ai` in the repo | notes-db `origin='cache'` rows | git-owned |
| Authorship notes (http backend, opt-in) | remote server (see `notes-backend-spec.md`) | notes-db `origin='queue'` (pending→uploaded cache) | uploaded rows kept as cache; cache evicted ≥90d over 10k rows |
| Working checkpoints | `.git/ai/working_logs/<base_commit>/` JSON | none (daemon FamilyState watermarks reference them) | migrated/renamed on HEAD moves; consumed by post-commit |
| Ref/worktree state | git itself | daemon `FamilyState` (in-memory, re-derivable), reflog cursor offsets | reconstructable |
| Transcript positions | streams DB (v4): per-session watermarks | — | advanced monotonically; sessions re-scanned from watermark |
| Transcript content | the agents' own transcript files (never copied) | redacted events → metrics DB (allowed repos only) | agent-owned |
| Metrics events | metrics DB (v5) | — | pruned by age; upload queue retries ≤6 then parks |
| Bash tool-use provenance | bash-history DB (v2) | — | pruned |
| Prompts / CAS queue | internal DB (v3, legacy — queue dormant) | — | candidate for removal in P9.5 |
| User config | `~/.git-ai/config.json` | `CONFIG` snapshot (per-process); daemon uses `Config::fresh()` | user-owned; `git-ai uninstall --purge` deletes |
| Credentials | OS keyring (flagged) or config `api_key` / `GIT_AI_API_KEY` | in-memory token cache | masked in all serialization |

Known multi-source tension (accepted + mitigated): under the sqlite-default
backend, a teammate's pushed `refs/notes/ai` update is only observed via the
read-fallback (which backfills `origin='cache'`) or `git-ai fetch-notes`;
`origin='local'` rows always win upserts. Backend switches go through
`git-ai notes migrate --to <backend>` — never by hand-editing stores.

---

## Per-DB authority

The five persistence stores, their module homes, and who may write them:

| Store | Path | Schema version | Owning module | Accessor form | Writers |
|---|---|---|---|---|---|
| Notes DB | `~/.git-ai/internal/notes-db` (or `GIT_AI_TEST_NOTES_DB_PATH` in tests) | notes-db v2 | `model/repository/notes_db.rs` | `NotesDatabase::global()` singleton (`OnceLock<Mutex<NotesDatabase>>`) | `SqliteNoteStore` (local-primary), `HttpNoteStore` (queue), `cache_synced_notes` (read-cache backfill) via `notes_store.rs` |
| Internal DB | `~/.git-ai/internal/db` | v3 | `model/repository/internal_db.rs` | `InternalDatabase::global()` singleton | daemon post-commit pipeline, CAS queue |
| Streams DB | `~/.git-ai/internal/streams-db` | v4 | `model/repository/streams_db.rs` | `StreamsDatabase` injected at daemon init | stream workers; `streams_db::update_watermark(&dyn WatermarkStrategy)` is the reference pattern for strategy injection |
| Metrics DB | `~/.git-ai/internal/metrics-db` | v5 | `model/repository/metrics_db/` | global singleton | telemetry worker, event emitters |
| Bash-history DB | `~/.git-ai/internal/bash-history-db` | v2 | `model/repository/bash_history_db.rs` | global singleton | bash tool-use checkpoint pipeline |

### Notes-backend authority statement (per `notes_backend.kind`)

- **`sqlite` (default)**: `notes-db` is authoritative (`origin='local'` rows, written via `upsert_local_note[s_batch]` through `SqliteNoteStore`). `refs/notes/ai` is a read-only legacy fallback; on a miss the raw content is backfilled as `origin='cache'` so subsequent reads are served from the db. `read_notes_batch` propagates refs errors (fail-closed for rewrite migration); `read_note` and `read_authorship` are `Option`-typed and swallow refs errors.

- **`http`**: The remote server is authoritative. `notes-db` serves as a write queue (`origin='queue'`, synced=0, written via `upsert_note[s_batch]` through `HttpNoteStore`) and a read cache (`origin='cache'`, synced=1, written via `cache_synced_notes`). `HttpNoteStore::write_note/write_notes_batch` call `telemetry_handle::submit_notes()` as an inline flush kick — a documented store→daemon coupling. `refs/notes/ai` is a transition-period read fallback only (errors swallowed, no backfill). Batch reads add a remote-fetch tier before the refs fallback.

- **`git_notes`**: `refs/notes/ai` in the repository is solely authoritative. `notes-db` is untouched by reads or writes under this backend. `GitNotesStore` delegates 1:1 to `pub(in crate::operations::git)` fns in `refs.rs`; the visibility wall is enforced by module placement in `operations/git/`.

- **`refs/notes/ai-display`**: Disposable derived state. Rebuilt from scratch (from the all-zeros SHA) by `materialize_notes_for_display` on each call; not authoritative for any fact.
