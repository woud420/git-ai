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
| `model/{api_types, working_log, authorship_log_serialization}` | Domain (impure) | see violations below |
| `model/repository/*` (5 dbs + sqlite helpers) | Persistence | singletons by design until P9.5 |
| `error/`, `repo_url.rs`, `uuid.rs`, `checkpoint_content_budget.rs` | Domain | pure |
| `feature_flags.rs` | Domain/config | reads `GIT_AI_*` env by design; P9 target: fold under `config/` |
| `config/*` | Orchestration (bootstrap) | one violation below |
| `clients/{api, auth, http}` | Network adapter | one violation below |
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
| `observability/` | Orchestration | one violation below |
| `notes/reference_server` | Test/reference infra | in-memory HTTP-contract server |

## Cross-layer violations (P9.2 work list)

1. `model/api_types.rs:2` → imports `operations::commands::diff::FileDiffJson` (DTO belongs in model).
2. `model/working_log.rs:2` → imports `Attribution`/`LineAttribution` from `operations::authorship::attribution_tracker` (types belong in model; tracker is pure and P9.3-bound anyway).
3. `model/authorship_log_serialization.rs:2` → takes `operations::git::repository::Repository` in signatures (split pure parse/serialize from repo-touching wrappers).
4. `model/repository/streams_db.rs:4` → imports `operations::streams::watermark::WatermarkStrategy` (watermark strategy types belong in model).
5. `clients/api/client.rs:5` → imports `operations::git::repository::Repository` (invert: pass what the client needs, not the repo).
6. `config/mod.rs:12` → imports `operations::git::repository::Repository` for `is_allowed_repository` (accept pre-fetched remotes/root instead, or move the repo-aware check out of config).
7. `observability/mod.rs` → imports daemon internals (invert or relocate).

## Ambient state access (outside `config/` and `cli/`)

- `feature_flags.rs:60,183-201,217,229` — `GIT_AI_*` env (by design; folds under config).
- `metrics/mod.rs:69` — `current_dir`.
- `diagnostics.rs:607,976` — `current_exe`.
- `observability/mod.rs:68` — `var_os`.
- Test-support env vars (`GIT_AI_TEST_*`) are cfg-gated and exempt.

## Duplicated / repeatedly-converted types

- `DiffHunk`: `operations/authorship/hunk_shift.rs` vs `operations/commands/diff.rs:37`.
- `ByteDiff` (`imara_diff_utils`) has no model-level representation; every consumer re-adapts.
- `Attribution`/`LineAttribution`: defined in the (pure) tracker, imported by `model/working_log` — resolved by the P9.3 move.

## Dissolution maps (no module may be named utils/helpers/common)

`utils.rs` (1,240 lines): `normalize_to_posix` + `unescape_git_path` → path
helpers next to their users (git adapter); git-exe discovery + terminal/
background-agent/superuser detection → `cli/`; `LockFile` → persistence
helper. `diagnostics.rs` (1,482): self-check orchestration → daemon;
trace2 validation → git adapter; blame formatting → commands; status helpers
→ control API. Both burn down opportunistically with the ratchet.
