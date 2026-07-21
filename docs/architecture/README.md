# Architecture

git-ai is a single binary dispatching on `argv[0]` (`src/main.rs`, `src/cli/`):
invoked as `git` it is a transparent proxy to the real git binary; invoked as
`git-ai` it serves direct subcommands. **All git integration is trace2-driven
and asynchronous** — nothing wraps git, and the ingestion path is
latency-sensitive (see the non-negotiables in the repo-root `AGENTS.md`).

Layer map (enforced direction; see `inventory.md` for per-module status):
`model` (pure domain + `model/repository` persistence) ← `clients`
(git_cli spawn layer, HTTP API, auth) ← `operations` (git adapter, daemon
orchestration, commands, integrations) ← `cli`.

## The canonical pipeline

```
Raw observation            trace2 JSON frames on the daemon socket; checkpoint
                           control requests; agent transcript files; reflog
                           byte-offsets captured at command start
        ↓
Normalized command         TraceNormalizer → NormalizedCommand
                           (operations/daemon/trace_normalizer.rs; GitBackend
                           seam resolves aliases/family, cached per family)
        ↓
Enrichment                 RefCursor::enrich_command fills exact ref_changes
                           from cursor-bounded reflog reads; fail-closed
                           (operations/daemon/ref_cursor/enrichment.rs)
        ↓
Classification             AnalyzerRegistry → SemanticEvent / AnalysisResult
                           (operations/daemon/analyzers/{generic,history,
                           workspace,transport}.rs) — IO-free
        ↓
Reduction                  reduce_family_command(state, cmd, analyzers)
                           → (AppliedCommand, AnalysisResult); pure in-memory
                           FamilyState transition (operations/daemon/reducer.rs)
        ↓
Effect execution           actor_coordinator_{side_effects,rewrites}.rs +
                           {git_op,side_effect}_helpers: post-commit note
                           generation, rewrite migration, working-log renames,
                           notes push/fetch sync — after exit_code == 0 only
```

Per-repository-family serialization: `coordinator.rs` routes by `CommandScope`
to one mpsc-fed actor per family (`family_actor.rs`) and a minimal global
actor; a per-family async exec-lock orders side-effect application
(`actor_coordinator_drain.rs`).

## The twelve questions

1. **Authoritative inputs** — trace2 frames (trace socket), checkpoint
   `ControlRequest::CheckpointRun` (control socket), agent transcript files
   (stream worker), reflog start-offsets captured at command boundary
   (`ref_cursor/reflog_io.rs`).
2. **Canonical event representation** — `model/domain.rs`:
   `NormalizedCommand` (observed fact) and `SemanticEvent` (interpretation).
3. **Where events are classified** — `operations/daemon/analyzers/*` only.
4. **Where attribution transitions are calculated** — `reducer.rs` for
   ref/worktree state; attribution content in `operations/authorship`
   (tracker pipeline is pure; virtual_attribution combines with git reads).
5. **Persistent state** — see `../contracts/persistence-model.md` (notes,
   working logs, five SQLite stores, config, credentials).
6. **Reconstructable state** — FamilyState refs/worktrees (re-derivable from
   git), notes cache rows (`origin='cache'`), alias cache, author-config
   cache, telemetry buffers.
7. **Serialized per repository** — everything flowing through the family
   actor: command reduction, checkpoint application, working-log mutation,
   side-effect execution. Not family-serialized: telemetry/metrics flush,
   notes upload queue (Mutex-guarded instead) — see
   `state-ownership.md`.
8. **Retryable effects** — notes uploads (attempts + `next_retry_at`
   backoff), metrics uploads (retry ≤ 6 attempts), daemon log uploads.
   Once-only: post-commit note generation, rewrite migrations, working-log
   renames (guarded by command sequencing).
9. **Idempotent effects** — notes push/fetch sync, cache warms, checkpoint
   re-application for identical content (upserts); note writes are
   last-writer-wins upserts in every backend.
10. **Git edge cases** — concentrated in `ref_cursor/{rebase_pull,
    cherry_pick_revert, stash_update_ref, span_clamping, command_matchers}`
    and the rewrite modules (`operations/authorship/rewrite*`,
    daemon `revert_rebase_helpers`, `cherry_pick_helpers`).
11. **OS differences** — socket transport (`socket_listeners.rs`: unix
    sockets vs named-pipe worker pool), path/registry handling in installers
    (`operations/commands/{install_hooks,uninstall}.rs`), `#[cfg]` pairs kept
    together by convention.
12. **Public compatibility boundaries** — the authorship note format
    (`specs/git_ai_standard_v3.0.0.md`, parsing in
    `model/authorship_log_serialization.rs`), the CLI + machine-readable
    output (`../contracts/cli-output.md`), the agent checkpoint interface
    (`../contracts/checkpoint-interface.md`), the notes HTTP backend wire
    contract (`../contracts/notes-backend-spec.md`). Internal Rust module
    visibility is NOT a compatibility guarantee.

## Deep-dive specs

- [daemon-trace2-ingestion-spec.md](daemon-trace2-ingestion-spec.md)
- [rewrite-ops-spec.md](rewrite-ops-spec.md)
- [state-ownership.md](state-ownership.md)
- [inventory.md](inventory.md)
