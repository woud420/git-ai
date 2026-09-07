# Telemetry Streams and Metrics Contract

**Status:** Current implementation as of 2026-09-06

**Privacy boundary:** [data-privacy.md](../../data-privacy.md)

**Persistence authority:** [persistence-model.md](persistence-model.md)

This document describes the stream cursors and metric-event pipeline implemented
in this fork. The dated documents under `docs/decisions/` record earlier designs;
they are not runtime contracts.

## Components and storage

| Component | Current responsibility |
|---|---|
| `StreamWorker` in `src/operations/daemon/stream_worker.rs` | Accept validated checkpoint notifications, run discovery sweeps, incrementally read agent-owned streams, redact secret-shaped values, and advance watermarks. |
| Stream adapters in `src/operations/streams/` | Discover supported agent sessions and decode each transcript or trace format in bounded batches. |
| Streams repository in `src/model/repository/streams_db.rs` | Store schema-v4 `tracked_streams` rows: stream identity, source path, format, watermark, file metadata, repository context, and processing errors. It does not copy transcript content. |
| Telemetry worker in `src/operations/daemon/telemetry_worker/` | Persist metric events and conditionally flush metrics and other configured telemetry destinations. Its regular flush interval is three seconds. |
| Metrics repository in `src/model/repository/metrics_db/` | Store schema-v5 compact event JSON, query metadata, delivery state, and local history. |

Production paths are beneath the configured git-ai home:

- stream cursors: `~/.git-ai/internal/transcripts-db`;
- metric events: `~/.git-ai/internal/metrics-db`.

The stream database keeps the historical `transcripts-db` filename for
compatibility with existing installations. `streams-db`, `transcripts.db`, and
top-level `metrics.db` are not paths opened by the current daemon.

## Processing flow

1. When an agent checkpoint supplies a stream source, the daemon validates that
   the path belongs to that agent and that the session maps to a repository in
   `allowed_repositories`. Valid notifications enter the immediate-priority
   queue.
2. With the `transcript_sweep` feature enabled, the worker also discovers stale
   streams at startup, every 30 minutes, and after eligible commit or push
   events. Triggered sweeps share a 30-second cooldown. Newly discovered files
   use the configured lookback, seven days by default; a value of zero means no
   lookback limit.
3. The adapter reads a bounded batch from the stored watermark. The worker
   assigns session and trace attributes, drops OTEL spans without an extractable
   session, and redacts secret-shaped JSON values.
4. The resulting `session_event` or `otel_trace` records are written to the
   metrics database before the watermark advances. The worker repeats until the
   adapter returns no more events, saving the watermark after every batch.
5. Local metric persistence is independent of remote delivery. Upload is
   attempted only when the master `telemetry` setting is on and an API key or
   login is available. Session events are checked against current repository
   eligibility again when dequeued for delivery.

The stream worker is created only while `transcript_streaming` is enabled.
`transcript_streaming` and `transcript_sweep` currently default to enabled in
debug and release builds, but an empty `allowed_repositories` list still denies
all repository transcript collection. The master `telemetry` setting defaults
to off. See the privacy boundary above before enabling any networked mode.

## Metrics event surface

The compact wire envelope is version 1 and uses `t` (timestamp), `e` (event
kind), `v` (position-encoded values), and `a` (position-encoded attributes).
Current event kinds are:

| ID | Event |
|---:|---|
| 1 | `committed` |
| 2 | `agent_usage` |
| 3 | `install_hooks` |
| 4 | `checkpoint` |
| 5 | `session_event` |
| 6 | `otel_trace` |
| 7 | `rewrite_committed` |

See [telemetry-examples.md](telemetry-examples.md) for anonymized payloads and
`src/model/metrics/types.rs` for the event-ID and envelope definitions.

## Failure and retention behavior

- Stream parse and fatal failures are recorded on the matching
  `tracked_streams` row. Transient processing failures use a bounded retry
  policy; there is no per-session one-second file poll.
- Metric rows are local history as well as an offline delivery queue. Retriable
  upload failures use row-level backoff and stop being eligible after six
  attempts; server-rejected rows remain as non-retryable history.
- Metric history has a 365-day retention window, with pruning considered at
  most once per 24 hours when rows are written or marked delivered.

Do not delete either database as routine troubleshooting. Removing the streams
database discards watermarks and can cause eligible agent-owned content to be
read again; removing the metrics database discards local history and queued
delivery state. A full intentional removal belongs to the documented
`git-ai uninstall --purge` lifecycle.

## Inspection and troubleshooting

`git-ai whoami` reports whether telemetry and metric delivery are enabled and
summarizes the local metrics queue. For lower-level inspection, stop the daemon
or use SQLite's read-only mode and inspect the current tables:

```bash
sqlite3 -readonly ~/.git-ai/internal/transcripts-db \
  "SELECT session_id, stream_kind, tool, watermark_value, processing_errors, last_error FROM tracked_streams;"

sqlite3 -readonly ~/.git-ai/internal/metrics-db \
  "SELECT COUNT(*) AS rows, SUM(delivered_ts IS NULL) AS pending FROM metrics;"
```

Useful implementation entry points are:

- daemon startup and store wiring: `src/operations/daemon/lifecycle.rs`;
- checkpoint stream authorization: `src/operations/daemon/checkpoint_stream_authority.rs`;
- discovery coordination: `src/operations/daemon/sweep_coordinator.rs`;
- stream processing: `src/operations/daemon/stream_worker.rs`;
- metric persistence and upload: `src/operations/daemon/telemetry_worker/metrics_flush.rs`.
