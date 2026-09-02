# Runaway Memory Remediation Plan (Pragmatic, Phased)

## Goal
Prevent `git-ai` from consuming runaway memory and long wall-clock time in heavy real-world sessions (long history, many files, large transcripts), while keeping checkpoint behavior correct.

## Measured Failure Signals
- Memory amplification in checkpoint history path:
  - `checkpoints.jsonl` ~307 MB produced ~1.78 GB peak RSS and ~33s runtime.
- Memory amplification in transcript parsing path:
  - transcript ~187 MB produced ~1.20 GB peak RSS and ~5.7s runtime.
- Amplification ratio repeatedly observed at ~5x-8x input size.

## Scope
- In scope:
  - checkpoint path performance and memory (`git-ai checkpoint`, commit pre-hook/post-hook code paths).
  - transcript parser memory behavior in agent presets.
  - regression guardrails and performance gates.
- Out of scope:
  - redesigning attribution semantics.
  - changing user-visible authorship data model in this first pass.

## Primary Hotspots (Code)
- `src/operations/git/repo_storage.rs` and
  `src/operations/git/repo_storage/checkpoint_journal.rs`
  - journal reads still materialize checkpoint history;
  - full-list mutations and periodic compaction still rewrite the collection.
- `src/operations/daemon/checkpoint.rs`
  - checkpoint history is indexed once per command and shared across per-file
    tasks; remaining work scales with the current changed-file set.
- `src/operations/commands/checkpoint_agent/presets/`
  - transcript parsers remain the owners of agent-specific JSONL loading.

## Phase 0: Immediate Safety Rails (1-2 days)
### Objectives
- Stop worst-case OOM/host-freeze before deeper refactors land.

### Changes
1. Add byte-size caps before parsing:
   - `max_checkpoint_jsonl_bytes`
   - `max_transcript_bytes`
2. Add per-command memory-safe fallback:
   - If cap exceeded, skip heavy enrichment path and continue checkpoint with warning.
   - Never hard-fail `git commit` because of oversized metadata.
3. Add explicit warning logs and one telemetry event for cap hits.

### Defaults (initial proposal)
- `max_checkpoint_jsonl_bytes`: 64 MB
- `max_transcript_bytes`: 32 MB

### Acceptance
- No OOM when running repro scenarios above cap.
- Commands complete with degraded metadata rather than crash/hang.

## Phase 1: Low-Risk Memory/Time Wins (2-4 days)
### Status (2026-09-02)
Implemented for the checkpoint path. A command reads history once, builds the
latest per-file state map once, and shares immutable maps with its per-file
tasks. Transcript-specific clone pressure remains tracked with its parsers; the
original phase-wide percentage targets have not been requalified here.

### Objectives
- Remove duplicate work and large unnecessary clones.

### Changes
1. Single checkpoint read per command:
   - Thread one parsed checkpoint set through checkpoint workflow.
   - Remove repeated `read_all_checkpoints()` calls in `get_all_tracked_files`.
2. Build per-file latest-entry index once:
   - Map `file -> latest checkpoint entry` once, then use O(1) lookup in per-file processing.
   - Avoid scanning all checkpoints for every file in `get_checkpoint_entry_for_file`.
3. Reduce clone pressure:
   - Avoid cloning `previous_checkpoints.to_vec()` into async task fanout.
   - Use shared immutable structures and lightweight references/indices.

### Acceptance
- For the same synthetic load, peak RSS down by >=30% on checkpoint scenario.
- Wall-clock down by >=25% on checkpoint-heavy scenario.

## Phase 2: Streaming Parsers (3-5 days)
### Status (2026-09-02)
Partially complete. Checkpoint JSONL is read with a buffered line reader, though
the resulting checkpoint collection is still materialized for attribution.
Transcript streaming is not claimed complete by this plan update.

### Objectives
- Eliminate full-file materialization for JSONL-heavy inputs.

### Changes
1. Stream transcript JSONL line-by-line:
   - Claude/Droid/Codex parser paths.
   - No `read_to_string` for large transcript files.
   - No intermediate `Vec<Value>` staging in Codex parser.
2. Stream checkpoint JSONL parsing:
   - Replace whole-file load in `read_all_checkpoints` with buffered line reader.
3. Keep behavior parity:
   - Same message extraction semantics and fallback logic.

### Acceptance
- Transcript scenario peak RSS down by >=40% at same input sizes.
- No correctness regressions in existing transcript-related tests.

## Phase 3: Storage Write-Path Refactor (4-7 days)
### Status (2026-09-02)
Partially complete. The default write path now durably publishes blobs before
appending a checksummed index record, so ordinary checkpoints no longer rewrite
the full journal. Synchronous compaction still occurs every 256 records, and
the read path still materializes history; a compact hot index and background
compaction remain future work.

### Objectives
- Remove full rewrite behavior as history grows.

### Changes
1. Introduce append-friendly checkpoint storage:
   - Keep append-only log for new checkpoints.
2. Add compaction strategy:
   - Background/triggered compaction for old checkpoint entries.
   - Preserve only required historical attribution granularity.
3. Isolate “hot working set” from cold history:
   - Fast path reads latest per-file state from compact index.
   - Avoid replaying entire history on each checkpoint.

### Acceptance
- Checkpoint runtime growth becomes near-linear in current changed files, not history size.
- Large-history scenarios no longer require reading/re-writing entire checkpoint corpus.

## Testing And Validation Plan
### Repro harness
- Use `scripts/repro_runaway_memory.py`.
- Keep two scenario suites:
  - `checkpoints`
  - `claude`

### Bench profiles (fixed)
1. `checkpoint-medium`: expected <250 MB peak RSS, <6s runtime.
2. `checkpoint-heavy`: expected <600 MB peak RSS, <15s runtime.
3. `claude-medium`: expected <200 MB peak RSS, <2s runtime.
4. `claude-heavy`: expected <450 MB peak RSS, <5s runtime.

### CI integration
- Add non-blocking perf job first (collect baseline over several runs).
- Promote to blocking after variance window is understood.

## Original Rollout Proposal
This sequence records the original proposal. The phase status notes above are
the current implementation record: checkpoint Phase 1 and parts of Phases 2
and 3 now run by default rather than behind feature flags.

1. Ship Phase 0 with conservative caps and warnings.
2. Monitor:
   - cap-hit rate
   - checkpoint duration p95/p99
   - crash/force-kill reports
3. Ship Phase 1 and Phase 2 behind feature flags.
4. Enable by default after stable canary period.
5. Ship Phase 3 last, with migration-safe fallback.

## Current Rollback Plan (2026-09-02)
- Transcript changes may continue to use their own flags where present.
- The checkpoint journal is not feature-flagged. Roll it back with a code
  revert and release; its versioned records retain the flat `Checkpoint` JSON
  shape so the legacy reader can ignore journal metadata.
- Keep the checkpoint byte cap and other safety rails enabled.

## Risks And Mitigations
1. Risk: metadata truncation surprises users.
   - Mitigation: explicit warning and docs; best-effort fallback behavior.
2. Risk: parser streaming changes transcript semantics.
   - Mitigation: golden fixtures + snapshot comparison before/after.
3. Risk: storage refactor causes data consistency bugs.
   - Mitigation: dual-read verification mode during rollout.

## Concrete Work Breakdown
1. Add config keys and cap checks in checkpoint/transcript read entry points.
2. Refactor checkpoint command flow to pass shared checkpoint context once.
3. Build per-file latest-entry index and replace repeated scans.
4. Convert Claude/Codex/Droid JSONL parsing to buffered streaming readers.
5. Add repro-driven benchmark test script to CI.
6. Complete the append+compact storage work with a bounded read-side index and
   off-acknowledgement compaction.

## Exit Criteria
- Repro harness can no longer produce runaway growth at previously failing inputs.
- Heavy scenarios stay inside agreed memory/runtime budgets.
- No correctness regressions in existing test suite and transcript fixtures.
