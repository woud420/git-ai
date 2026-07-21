# P9 — Layered Architecture on the Kickstart Structure

Adapted from an external "explicit architectural layers" plan; grounded against the actual post-restructure codebase (see `../architecture/inventory.md` for the current per-module status). Execution began 2026-07-21 with P9.1 (this document's landing PR).

Optimize for one property (unchanged from the source plan):
> For any behavior, it should be clear which input fact triggered it, which deterministic rule decided the result, and which adapter performed the side effect.

## What is ALREADY true (verified 2026-07-20, do not rebuild)

- **The canonical pipeline exists**: trace2 frames → `TraceNormalizer` → `NormalizedCommand` (observed fact) → `daemon/analyzers/*` (`CommandAnalyzer::analyze` → `AnalysisResult`/`SemanticEvent`, classification) → `reducer.rs` (`reduce_family_command(state, cmd, analyzers) -> (AppliedCommand, AnalysisResult)`, pure, returns effect-like data) → side-effect application in the daemon.
- **Per-repository serialization exists**: `coordinator.rs` routes by `CommandScope` to per-family mpsc actors (`family_actor.rs`) and a global actor — GPT's "one family → one ordered queue → one reducer stream" is the current design.
- **The notes I/O choke point exists**: `operations/git/notes_api.rs` is the single dispatcher for all authorship-note reads/writes (sqlite/git-notes/http); no bypass writers found.
- **Half the core is already pure**: `model/{authorship_log, domain, stream_types}` are pure; `operations/authorship/{attribution_tracker, hunk_shift}` are pure attribution algebra in the wrong directory.

## Layer mapping (kickstart dirs, NO renames — do not create src/core, src/git_adapter, src/app)

| GPT layer | Kickstart home | Rule |
|---|---|---|
| Pure core domain | `src/model` **excluding** `model/repository` | imports nothing from operations/cli/clients/config/model::repository; no tokio/rusqlite/fs/env/process/sockets |
| Persistence adapter | `src/model/repository` | may import model; owns schemas, migrations, transactions, singletons |
| Git adapter | `src/operations/git` | may import model (+config); converts git facts ↔ domain events/effects; no domain policy |
| Network adapters | `src/clients` | may import model, config |
| Orchestration | `src/cli`, `src/operations/{daemon,commands}` | may import everything below; thin handlers |
| Agent/IDE integrations | `src/operations/{mdm,streams,ci}` | adapters; may import model + operations::git |

Forbidden: model(pure) → anything above it; operations::git → cli/commands/daemon. No modules named common/shared/utils/helpers/misc (existing `utils.rs` is P8 burn-down scope — dissolve it into owners, don't bless it).

## Stages (each = 1–3 PRs, base on main, template PR bodies, full task-test gate)

**P9.1 — Inventory + canonical docs.** `docs/architecture/inventory.md` (per-module classification: Domain / Git adapter / Persistence / Orchestration / Interface / Mixed, with the mixed-responsibility and cross-layer-import lists), rewrite `docs/architecture/README.md` as the pipeline above with the 12 GPT questions answered, `docs/architecture/state-ownership.md` (every OnceLock/static: CONFIG, AUTHOR_CONFIG_CACHE, DISTINCT_ID, the 5 db singletons, daemon telemetry statics, DAEMON_PROCESS_ACTIVE — owner/scope/mutation path/recovery), `docs/contracts/persistence-model.md` (source of truth per fact: notes vs working logs vs sqlite vs daemon memory), plus `docs/contracts/{cli-output.md, checkpoint-interface.md}` for the public boundaries (note format is already `specs/git_ai_standard_v3.0.0.md`). Must resolve definitively: whether any attribution write path escapes the family-serialized daemon flow (the hook-handler path vs daemon side-effect path).

**P9.2 — Purify model + enforcement.** Fix the three verified leaks: (a) `model/working_log.rs` imports `Attribution/LineAttribution` from `operations::authorship::attribution_tracker` — those TYPES move into model; (b) `model/api_types.rs` imports `FileDiffJson` from `operations::commands::diff` — the DTO moves into model; (c) `model/authorship_log_serialization.rs` takes `operations::git::repository::Repository` in signatures — split so pure parse/serialize stays, repo-touching wrappers move to operations. Then add `tests/integration/layer_import_policy.rs` (same pattern as file_length_policy): scans `use crate::` per layer and fails on forbidden direction. This is GPT's "dependency enforcement" as a 100-line policy test, not a framework.

**P9.3 — Move pure attribution algebra into model.** `attribution_tracker.rs` and `hunk_shift.rs` relocate (already pure); then peel pure kernels out of the three entangled modules (`virtual_attribution`, `range_authorship`, `rewrite` — each calls notes_api/git/tokio directly) one seam at a time: pure computation → model with unit tests, IO shell stays in operations. No big-bang rewrite; characterization tests before each move (GPT Phase 11).

**P9.4 — Effects-as-data, scoped.** Do NOT retrofit an Effect enum everywhere. Two targeted applications: (a) enforce (via the layer policy test) that `daemon/{reducer,analyzers}` stay IO-free — they already are; (b) at the rewrite/note-migration boundary, batch note writes already look like declared effects (`Vec<(sha, note)>` → `write_notes_batch`) — name that boundary explicitly and route the remaining direct `write_note` calls in rewrite paths through it. Handlers keep the GPT shape: observe → normalize → reduce → execute effects, which is the existing daemon flow made explicit.

**P9.5 — Ports + persistence separation (folds in the standing sqlite-abstraction request).** Introduce traits ONLY at real seams: `AuthorshipNoteStore` (the three notes backends behind notes_api become impls; notes_api the concrete dispatcher), and a narrow store trait per DB only where tests need substitution. Kill direct `::global()` singleton reaches from orchestration code — handles injected at daemon/CLI init; singletons stay at the edge, no DI container. Persistence row types stop doubling as domain types where they currently do (`TryFrom`/`From` conversions at the boundary). Document per-DB authority in persistence-model.md.

**P9.6 — Error normalization.** Layered error types: `DomainError` (model), keep `GitCliError` family (git adapter), `PersistenceError` (model/repository), `CommandError` (cli, user-facing conversion). Burn down the **443 `GitAiError::Generic(String)`** uses opportunistically per touched module — never a repo-wide sweep; retain operation + identifier + source + retryability.

**P9.7 — Crate extraction, optional and last.** Only after layer_import_policy has been green across several PRs and the model API is curated (`pub use` root, private-by-default): extract `crates/git-ai-core` (= model minus repository; manifest with zero infra deps — no tokio/rusqlite/ureq/interprocess/dirs). `git-ai-git` only if it demonstrably reduces coupling. Kickstart-compatible workspace. Explicit non-goal before then; do not extract to satisfy a diagram.

## Guardrails (adopted from source plan + repo rules)

No internal frameworks, DI containers, event buses, trait-per-struct, or control-flow macros. Prefer concrete types over single-impl traits. Private by default; `pub(crate)` for repo-internal; curated `pub use` roots. Temporary shims must carry why/callers/removal-condition and a tracked task. Delete obsolete paths after migration. Plus the non-negotiables: trace2-driven async only, nothing new on the ingestion hot path, no per-item git spawns, TestRepo TDD, `task test` (never raw cargo test on integration), ratchet baseline only shrinks (and the ratchet machinery itself is removed when the baseline empties — user directive).

## Definition of done (adapted)

Attribution rules unit-testable without git/SQLite/IPC/tokio; git behavior isolated behind operations/git; cli+daemon handlers orchestrate; one documented owner per mutable state; one documented source of truth per persisted fact; reducers/analyzers IO-free and enforced; dependency direction acyclic and policy-tested; internal APIs private by default; no oversized integration-bus file (P8's job); supported behavior covered by the existing suite + new core unit tests; migration shims removed or tracked. A new engineer traces a command from input → event → transition → effects without searching the whole repo.
