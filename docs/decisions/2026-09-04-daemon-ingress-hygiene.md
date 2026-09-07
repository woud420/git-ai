# Daemon ingress hygiene

Status: accepted.

## Decision

Reclaim a trace root after 30 minutes without a frame, on the existing
30-second socket-health cadence. Every frame already refreshes the root's last
activity time. The health thread scans for idle roots only when that root has
no queued payloads, then sends the existing synthetic connection-close event
through the bounded trace-ingest queue. Mutating roots therefore take the same
cleanup path as a disconnected client: the normalizer state is swept, the
`PendingRoot` sequencer entry becomes `Canceled`, the per-root ingress state is
cleared, and the affected family drain resumes.

The close marker carries the activity timestamp observed by the scan. If a
frame refreshes the root before the marker is applied, the marker is ignored
and the root remains fenced. This keeps the fail-closed bias for active Git
operations without adding work to ordinary trace ingestion. The 30-minute
timeout is deliberately much longer than the 30-second scan cadence so a
briefly quiet rebase is not treated as abandoned.

## Bounded-state ledger

| Surface identified by the ENG-302 audit | Disposition |
| --- | --- |
| `TraceIngressState` root maps | Root terminal events and connection closes already clear every map. The idle reaper now gives abandoned roots the same cleanup path after 30 minutes. Roots with queued payloads are never selected. |
| `pending_root_slots_by_root` and family sequencer placeholders | The idle close marker reuses `replace_pending_root_entry(..., Canceled)`, removes the slot, and schedules the family drain. Empty sequencer maps continue to be removed by `gc_stale_family_state`. |
| Trace ingest queue, reorder buffer, and per-root queued counts | The queue and reorder buffer share the hard `TRACE_INGEST_QUEUE_CAPACITY` of 16,384 and shut the daemon down on overflow. Per-root counts are removed when processing finishes or root tracking is cleared. |
| Telemetry event vectors | Every best-effort in-memory event kind now retains at most 5,000 newest entries. Metric submissions normally bypass this buffer for bounded SQLite batches. |
| Stream-worker checkpoint channel | Remains intentionally unbounded. Each notification carries checkpoint-specific stream identity, and silently dropping or merging notifications would weaken token/session attribution. Making the producer async would move stream-worker backpressure onto acknowledged checkpoint processing. The worker drains notifications before completion barriers, normal sweeps are coalesced, and the daemon memory watchdog remains the process-level escape hatch. |
| Stream-worker sweep channel | Remains structurally unbounded, with admission bounded by the shared 30-second `SweepTriggerGate` for normal triggers. Recovery triggers carry a completion responder and must not be silently discarded. |
| Stream-worker drain channel | Remains structurally unbounded because each entry is a completion-bearing causal barrier. A channel cap would retain the same waiters in control tasks without reducing total live state; the control request size and response wait are already bounded. |

The three stream-worker channels are not part of trace2 ingestion. Replacing
their lossless contracts needs a separate durable/coalescing design rather
than an arbitrary queue limit in this patch.

## Post-v1.6.24 upstream hardening disposition

The upstream `3c41054e` merge and its side commits were evaluated behavior by
behavior rather than cherry-picked.

| Upstream behavior | Fork disposition |
| --- | --- |
| Early unconsumed-frame filtering (`648f8e92`) | Reject in this change. The fork's normalizer already ignores unconsumed event kinds and the ingest queue fails closed at a fixed capacity. Moving the filter into socket readers would change the latency-critical path and would also hide the otherwise-useful activity frames that keep a long-running root alive. It needs independent performance and liveness evidence before adoption. |
| End-to-end drain probe (`6a7d25a7`) | Reject in this change. The fork retains its bounded socket connect probe, bootstrap receive timeout, queue-full shutdown, and worker-error shutdown. A synthetic parser probe changes the trace wire path and does not replace the idle-root state recovery implemented here. |
| Sliding restart budget (`6a7d25a7`) | Present with a different, stricter bound. The fork restarts only after the process has survived the minimum-uptime gate; an earlier failure stops without another automatic restart. This bounds crash loops without a persistent restart ledger. |
| Ingest-loss metrics (`7e71f693`) | Present for the fork's failure contract. Queue closure and overflow emit structured reason-tagged errors and immediately request shutdown instead of continuing after known trace loss. Extra drop counters and a `stats.ingest` endpoint are rejected because no successful attribution is reported after that failure boundary. |

## Verification contract

Daemon-mode coverage opens a real trace socket, registers a mutating root, and
keeps the connection open. Periodic non-terminal frames keep its family fence
closed beyond the test timeout; once those frames stop, the health loop reaps
the root and the same family sync succeeds. Focused buffer coverage verifies
that the oldest best-effort telemetry entries are dropped at the cap.
