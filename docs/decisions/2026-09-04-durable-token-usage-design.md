# Fork-Native Durable Token-Usage Ingestion

**Date:** 2026-09-04  
**Status:** Proposed  
**Tracks:** ENG-323, ENG-278

## Decision

Adopt durable token-usage ingestion as a fork-native, local-only pipeline. The
pipeline will normalize supported transcript records into dedicated tables in
the existing metrics database, maintain restart-safe source cursors and
deduplication state in the same transaction, and serve `git-ai usage` from
durable five-minute aggregates.

This is not a direct port of upstream's TokenUsage telemetry event. In
particular:

- no token-usage event is added to the hosted telemetry wire format;
- no token-usage row enters the generic metrics upload queue;
- no parsing or storage work runs on the trace2 ingestion path;
- no raw prompt, response, or transcript content is copied into the normalized
  token tables; and
- backfill uses the bounded reingestion control surface planned by ENG-317,
  while retaining token-specific cursors and deduplication state.

The durable pipeline replaces token aggregation from raw `SessionEvent` rows as
the single source of truth for the token and cost sections of `git-ai usage`.
Raw session events remain authoritative for other session statistics until
their own migrations are designed.

## Context

Today `git-ai usage` reads event kinds 1, 4, and 5 from the metrics database and
folds token fields from raw `SessionEvent` JSON in memory. Claude usage is
deduplicated by assistant message ID and Codex cumulative counters are reduced
to per-session maxima. This works for a bounded query window, but it does not
provide a durable parser cursor, restart-safe replacement semantics, a bounded
historical rebuild, or an independently retained token aggregate.

Upstream split its second-generation work across parser, SQLite, metric-schema,
daemon-worker, end-to-end, and throttling changes. That decomposition contains
useful invariants, but its emitted TokenUsage telemetry event and separate
token database do not fit this fork's local-stats and privacy boundaries.

## Current and Upstream Responsibility Map

| Responsibility | Fork today | Upstream v2 evidence | Fork-native disposition |
| --- | --- | --- | --- |
| Claude parsing | Query-time extraction from raw session events | Incremental parser with strict fixtures in upstream PR #2206 | Reuse the parser behavior, adapted behind a source-neutral parser trait |
| Codex parsing | Query-time per-session cumulative maxima | Persisted cumulative state and deltas in upstream PR #2206 | Persist source state so restarts cannot double count |
| Durable cursor and dedupe | Transcript watermark exists, token state does not | Atomic cursor, parser state, and entry commit in upstream PR #2207 | Store in the existing metrics DB transaction |
| Aggregate storage | Recomputed in memory for every usage query | Five-minute bucket state in upstream PRs #2207 and #2208 | Store local five-minute buckets; do not create an upload event |
| Worker lifecycle | Stream worker persists raw session events | Notification, sweep, retry, and backpressure work in upstream PR #2209 | Add a separate bounded token worker outside trace2 ingestion |
| Historical rebuild | Query only over already persisted raw events | Initial scan and bounded batches in upstream PRs #2209 and #2215 | Reuse ENG-317's CLI lifecycle and progress contract |
| Retention | Generic metric rows retained for approximately 365 days | Token entry pruning added in upstream PR #2210 | Keep entries 90 days and aggregates 365 days |
| Pricing | Query-time model matching in `local_stats/tokens.rs` | Cached price lookup in upstream PR #2206 | Use the deterministic catalog delivered by ENG-314 |
| Export | Generic metric rows can enter the upload queue | TokenUsage metric ID 9 in upstream PR #2208 | Reject for this project; reconsider only in a separate privacy review |

The upstream pull requests are design evidence, not patch dependencies:
[#2206](https://github.com/git-ai-project/git-ai/pull/2206),
[#2207](https://github.com/git-ai-project/git-ai/pull/2207),
[#2208](https://github.com/git-ai-project/git-ai/pull/2208),
[#2209](https://github.com/git-ai-project/git-ai/pull/2209),
[#2210](https://github.com/git-ai-project/git-ai/pull/2210), and
[#2215](https://github.com/git-ai-project/git-ai/pull/2215).

## Required Behavior

| Surface | Contract | Failure behavior | Required proof |
| --- | --- | --- | --- |
| Parser inputs | Initially support Claude JSONL and Codex JSONL through explicit, versioned parsers | Unknown or malformed records are skipped with bounded diagnostics; a partial trailing record is retried | Sanitized fixtures cover valid, malformed, partial, appended, and truncated files |
| Durable identity | Identify a logical entry from tool, source session identity, stable message/item identity, and entry kind; hash the canonical identity before storage | A record without a safe stable identity does not advance the committed cursor | Copy/rename and resumed-session tests produce one logical entry |
| Deduplication | Enforce the logical-entry hash globally, not merely per file, and support corrected records replacing prior values | A replay is a no-op; a replacement adjusts both its old and new bucket in one transaction | Tests copy histories, restart, replay, correct, and move timestamps across buckets |
| Cursor and parser state | Commit source cursor, cumulative parser state, normalized entries, and bucket deltas atomically | Any parse or database failure leaves the prior cursor reusable | Fault-injection tests fail each write stage, restart, then converge exactly once |
| Backfill | Expose an explicit, cancelable command with persisted progress, bounded batches, and retry-safe resume | Interruption reports the last durable boundary and never restarts from zero unless requested | `TestRepo` lifecycle test interrupts, resumes, and compares with a clean run |
| Worker lifecycle | Notifications are prioritized over sweeps; work is off the trace2 path and has fixed queue, batch, memory, and concurrency bounds | Queue pressure coalesces source keys; failures back off without blocking checkpoints or shutdown | Dedicated-daemon tests exercise pressure, retry, restart, sync, await, and shutdown |
| Event schema | Store integer token counts, integer cost in micro-USD, model/catalog identity, source timestamps, five-minute bucket, and a local revision | Overflow or schema mismatch fails closed; no floating-point value is persisted | Round-trip and migration tests prove stable JSON output and cost rounding |
| Retention | Retain normalized entries for 90 days and aggregate buckets for 365 days; prune in bounded daily batches | A failed prune is retryable and never blocks ingestion | Clock-controlled tests verify boundaries and bounded progress |
| Privacy | Store counts and opaque hashed identities only; never store prompts, responses, tool arguments, file paths, or raw transcript JSON in token tables | Disallowed repositories are neither read nor backfilled; revoked eligibility prevents later processing | Tests cover allowed, missing, revoked, and excluded repository contexts |
| Export | Token tables and aggregates remain local and cannot be dequeued by metrics upload code | There is no remote fallback | A test proves token rows are absent from upload batches |

## Storage and Transaction Boundary

Use the existing metrics SQLite file so lifecycle, migration, locking, test
isolation, backup expectations, and the 365-day local-history policy remain in
one place. Add dedicated tables rather than serializing derived usage as a
`MetricEvent`:

- `token_usage_sources`: source kind, opaque source identity, parser version,
  committed cursor, cumulative parser state, last observed metadata, and retry
  state;
- `token_usage_entries`: canonical identity hash, source reference, bucket,
  model/catalog identity, token counts, reasoning count when available,
  micro-USD cost, source timestamp, and replacement revision; and
- `token_usage_buckets`: five-minute key plus model/catalog identity, summed
  counts and micro-USD, logical message count, and monotonic local revision.

The concrete migration may normalize repeated columns differently, but these
three responsibilities and constraints are fixed. Foreign keys and unique
indexes must make it impossible to commit an entry without its source or to
store the same logical entry twice.

For every batch, one SQLite transaction must:

1. validate and normalize the new source records;
2. remove prior bucket contributions for replacements;
3. insert or replace normalized entries;
4. apply the resulting bucket deltas; and
5. advance the cursor and parser state.

The transaction commits only after every step succeeds. Source file I/O and
parsing occur before the write lock is acquired, with a bounded batch retained
in memory.

## Identity and Replacement Rules

Provider event IDs are used when their documented scope is stable. Otherwise a
canonical identity is built only from non-content metadata that is stable
across replay. The stored key is a domain-separated hash containing parser
version, tool, source session identity, logical entry kind, and stable entry
identity.

Content hashes are fingerprints for detecting corrected records; they are not
the logical key. When the same logical key arrives with a different
fingerprint, the new normalized value replaces the old one and the transaction
reverses the old bucket contribution before applying the new contribution.
This permits corrected counts and timestamps to decrease totals as well as
increase them.

Codex cumulative reports additionally persist the last accepted cumulative
state per source. Negative deltas caused by a reset begin a new epoch rather
than subtracting a prior epoch. Parser fixtures must define each accepted reset
shape before implementation.

## Worker, Backfill, and Query Flow

The daemon owns one bounded token worker. Checkpoint/session notifications only
enqueue or coalesce an opaque source key; they perform no transcript reads,
parsing, Git work, or SQLite work on the critical ingestion path. A periodic
sweep supplies low-priority catch-up work. The implementation must publish
explicit constants for queue capacity, batch record count, batch byte count,
concurrency, retry budget, and sweep interval.

ENG-317 owns the user-facing reingestion lifecycle: command naming, progress,
cancellation, resume, and bounded repository/object traversal. Token usage
registers as one reingestion family and persists its own source cursor in the
token tables. It must not share a generic cursor whose advancement could make
one metric family skip another.

During rollout, `git-ai usage` reads both implementations only in a debug/test
comparison mode. Release output continues to use the legacy raw-event fold
until the durable pipeline has caught up for the requested window. Cutover is
atomic: release output then reads only durable buckets. It never sums legacy
and durable totals. The legacy token fold can be removed after a one-release
fallback window and a migration test from the prior metrics schema.

## Retention and Privacy Decisions

Normalized entries are retained for 90 days, which is long enough to replay,
correct, and audit recent aggregates without indefinitely retaining
message-level activity. Five-minute aggregates are retained for 365 days to
match the existing local usage-history statement. Source cursor rows remain
while their source exists and become tombstones for 90 days after disappearance
so rediscovery cannot duplicate retained entries.

The normalized tables contain no transcript content. Stable external IDs are
hashed before storage, repository eligibility is checked before reads, and
derived rows never enter the metrics retry/upload queue. Hosted telemetry is
explicitly out of scope. Any future export requires a separate issue covering
consent, aggregation granularity, schema compatibility, deletion, and a new
privacy review.

## Ordered Implementation Issues

After this decision is accepted, create the following focused Linear children
under ENG-323. Creating them is deliberately separate from merging this design
so reviewers can adjust boundaries without leaving stale tracker work.

| Order | Proposed issue | Depends on | Minimum `TestRepo` acceptance evidence |
| --- | --- | --- | --- |
| 1 | Parse Claude and Codex token records into a canonical entry model | None | Sanitized transcripts prove append, partial record, malformed record, cumulative delta, reset epoch, and stable identities |
| 2 | Add durable token sources, entries, and buckets to the metrics DB | 1 | Faulted batch writes and daemon restarts prove atomic cursor/state/dedupe/replacement behavior and schema migration |
| 3 | Add bounded token-usage reingestion | 2 and ENG-317's lifecycle contract | Interrupted multi-source backfill resumes with bounded batches and equals a clean run without duplicate totals |
| 4 | Run durable token ingestion in the daemon | 2 | Notification/sweep pressure proves fixed bounds, priority, coalescing, backoff, restart, sync/await, and prompt shutdown with no trace-path I/O |
| 5 | Cut `git-ai usage` over to durable token buckets | 2, 3, 4, and ENG-314 | Legacy/durable comparison fixtures reconcile exactly, pricing/catalog rounding is stable, and the release path reads one source only |
| 6 | Enforce token retention and local-only privacy boundaries | 2 and 4 | Clock-controlled pruning stays bounded; disallowed/revoked repos are skipped; upload batches contain no token rows |

Issues 3 and 4 may proceed in parallel after storage lands. Issue 5 is the
cutover gate. Issue 6 may be developed alongside the worker but must land
before cutover.

## Alternatives Rejected

### Port upstream v2 unchanged

Rejected because it introduces a separate database and an uploadable wire
event while the fork already has a metrics database, a local usage surface, and
a stricter repository eligibility boundary.

### Keep query-time aggregation as the only implementation

Rejected because it cannot provide restart-safe parser state, replacement
semantics, bounded backfill, or predictable query cost without repeatedly
reading raw session-event payloads.

### Emit a local TokenUsage `MetricEvent` marked delivered

Rejected because delivery state is not a privacy type. Reusing the generic
event table would make local-only behavior depend on every dequeue and future
migration preserving a special case. Dedicated tables make export impossible
by construction.

### Share one backfill cursor across metric families

Rejected because independently retrying or adding a family could advance past
work another family has not durably consumed. The CLI lifecycle is reusable;
family cursors are not.

## ENG-278 Disposition

Adopt the capability through the six focused changes above. Do not take the
upstream monolithic port or its hosted TokenUsage event. ENG-323 is complete
when this decision is accepted and the agreed child issues have been created;
the capability itself remains tracked by those children.
