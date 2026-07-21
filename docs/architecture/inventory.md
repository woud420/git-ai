# Architecture Inventory

Snapshot of every `src/` module classified by layer, with the known violations
of the intended dependency direction. Produced 2026-07-21 (post-restructure,
post file-length burn-down); update alongside layer-moving PRs. The intended
direction and the plan that consumes this inventory live in
`../decisions/2026-07-20-layered-architecture-plan.md`.

Layers: **Domain** (pure logic/types) · **Git adapter** · **Persistence
adapter** · **Network adapter** · **Orchestration** · **Interface**
(dispatch/CLI) · **Integration adapter** (agent/IDE) · **Mixed**.

## Classification

| Module | Layer | Notes |
|---|---|---|
| `model/{authorship_log, domain, stream_types}` | Domain | pure |
| `model/{attribution, diff_json, stream_watermark, telemetry}` | Domain | pure DTOs/value types moved in during P9.2 |
| `model/{api_types, working_log, authorship_log_serialization}` | Domain | P9.2 leaks resolved (see below); `api_types` retains one documented residual (`transcript::Message`, P9.3) |
| `model/repository/*` (5 dbs + sqlite helpers) | Persistence | singletons by design until P9.5 |
| `error/`, `repo_url.rs`, `uuid.rs`, `checkpoint_content_budget.rs` | Domain | pure |
| `feature_flags.rs` | Domain/config | reads `GIT_AI_*` env by design; P9 target: fold under `config/` |
| `config/*` | Orchestration (bootstrap) | still imports `Repository` for the prompt-storage checks; the `is_allowed_repository` wrapper moved out in P9.2 (see below) |
| `clients/{api, auth, http}` | Network adapter | dependency-clean after P9.2: no `operations/` imports |
| `clients/git_cli/` | Git adapter (spawn layer) | dependency-clean: no `operations/` imports |
| `operations/git/*` | Git adapter | `repository/`, `refs`, `notes_api` (notes choke point), `status`, `repo_state`, `repo_storage`, `sync_authorship`, `fast_reader`, `cli_parser`, `command_classification`, `authorship_traversal` |
| `operations/daemon/*` | Orchestration | actors/coordinator/reducer/analyzers/ref_cursor + socket listeners; reducer + analyzers are IO-free (keep it that way) |
| `operations/commands/*` | Orchestration | command handlers; several still oversized (see `.file-length-baseline.txt`) |
| `operations/authorship/*` | Mixed by design | `attribution_tracker/` + `hunk_shift` are pure Domain (P9.3 move candidates); `virtual_attribution/`, `range_authorship`, `rewrite*` entangle computation with git/notes IO |
| `operations/{mdm, streams, ci}` | Integration adapter | agent/IDE installers, transcript readers, CI context |
| `cli/*`, `main.rs` | Interface | argv[0] dispatch is load-bearing |
| `metrics/{types, events, attrs, pos_encoded, local_stats}` | Domain + Orchestration mix | event structs are model material; emission/local-stats are orchestration |
| `tokio_runtime.rs`, `process_timeout.rs`, `http.rs`(clients) | Infrastructure glue | |
| `diagnostics.rs`, `utils.rs` | **Mixed** | dissolution maps below |
| `observability/` | Orchestration | telemetry DTO leak resolved in P9.2; still dispatches to daemon submit fns (orchestration→orchestration, allowed) |
| `notes/reference_server` | Test/reference infra | in-memory HTTP-contract server |

## Cross-layer violations (P9.2 work list — RESOLVED)

All seven are addressed by the P9.2 layer-purity PR. The intended direction is
now enforced by `tests/integration/layer_import_policy.rs` (scans `use crate::…`
per layer; see [Enforcement](#enforcement)).

1. ✅ `model/api_types.rs` imported `operations::commands::diff::FileDiffJson`.
   `FileDiffJson` (+ its serializer) moved to new `model/diff_json.rs`; `diff.rs`
   and `api_types.rs` now import it from model (no re-export).
2. ✅ `model/working_log.rs` imported `Attribution`/`LineAttribution` from
   `operations::authorship::attribution_tracker`. Both types (+ impls) moved to
   new `model/attribution.rs`; the tracker keeps a curated `pub use` re-export
   sourced from model, so its many operations-layer users stay valid.
3. ✅ `model/authorship_log_serialization.rs` took `&Repository`. The
   repo-touching `get_line_attribution` (git-notes fallback) moved to
   `operations/authorship/line_lookup.rs` as a free function; pure parse/
   serialize stays in model. `blame.rs` callers flipped.
4. ✅ `model/repository/streams_db.rs` imported
   `operations::streams::watermark`. The whole module moved to
   `model/stream_watermark.rs` (git-mv, same ratchet ceiling under the new
   path); all src/test users flipped; `streams/mod.rs` re-exports from model.
5. ⚠️ CORRECTED: `clients/api/client.rs` never imported
   `operations::git::repository::Repository` — the documented type import did
   not exist. It *did* import two identity helper *functions*
   (`current_git_committer_identity_resolution`, `parse_git_var_identity`),
   which is still a `clients → operations` leak. Inverted: the git-identity
   resolution moved to `operations::git::repository::resolve_api_author_identity`
   and the `ApiContext` constructors now accept an
   `AuthorIdentityResolver = fn() -> Option<String>` (orchestration callers pass
   the git-adapter resolver). `clients/**` is now free of `operations` imports.
6. ✅ `config/mod.rs` imported `Repository` for `is_allowed_repository`. The
   pure `is_allowed_repository_with_context(remotes, repo_root)` stays in config;
   the Repository-consuming wrapper moved to
   `impl Repository { fn is_collection_allowed(&self, &Config) -> bool }` in the
   git adapter. The ~4 gate call sites (checkpoint gate, daemon commit/amend
   gates, stream-worker transcript gate) flipped, preserving the
   `has_allowed_repositories` fast path. (config still imports `Repository` for
   `should_exclude_prompts` / `effective_prompt_storage`; config is not covered
   by the enforced layer rules.)
7. ✅ `observability/mod.rs` used `operations::daemon::TelemetryEnvelope` in a
   pub fn signature. The `TelemetryEnvelope` DTO moved to `model/telemetry.rs`
   (composed of `metrics::MetricEvent` + serde types); observability and the
   daemon telemetry modules import it from model. `daemon.rs` re-exports it from
   model for the existing `operations::daemon::TelemetryEnvelope` path.

### Enforcement

`tests/integration/layer_import_policy.rs` fails on forbidden `use crate::…`
directions:
- `src/model/**` (excluding `src/model/repository/**`): no `use crate::{operations,
  cli, clients, config}` and no `use {tokio, rusqlite}`.
- `src/model/repository/**`: no `use crate::{operations, cli}`.
- `src/clients/**`: no `use crate::operations`.

One documented residual is listed in the test's `ALLOWED_EXCEPTIONS`:
`model/api_types.rs`'s `CasMessagesObject` holds
`Vec<operations::authorship::transcript::Message>`; relocating the transcript
`Message` type is P9.3 scope, not P9.2.

## Ambient state access (outside `config/` and `cli/`)

- `feature_flags.rs:60,183-201,217,229` — `GIT_AI_*` env (by design; folds under config).
- `metrics/mod.rs:69` — `current_dir`.
- `diagnostics.rs:607,976` — `current_exe`.
- `observability/mod.rs:68` — `var_os`.
- Test-support env vars (`GIT_AI_TEST_*`) are cfg-gated and exempt.

## Duplicated / repeatedly-converted types

- `DiffHunk`: `operations/authorship/hunk_shift.rs` vs `operations/commands/diff.rs:37`.
- `ByteDiff` (`imara_diff_utils`) has no model-level representation; every consumer re-adapts.
- `Attribution`/`LineAttribution`: now owned by `model/attribution.rs` (moved in P9.2); the tracker re-exports them for its downstream users.

## Dissolution maps (no module may be named utils/helpers/common)

`utils.rs` (1,240 lines): `normalize_to_posix` + `unescape_git_path` → path
helpers next to their users (git adapter); git-exe discovery + terminal/
background-agent/superuser detection → `cli/`; `LockFile` → persistence
helper. `diagnostics.rs` (1,482): self-check orchestration → daemon;
trace2 validation → git adapter; blame formatting → commands; status helpers
→ control API. Both burn down opportunistically with the ratchet.
