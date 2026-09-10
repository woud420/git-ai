# Incremental native admission and bounded prefix replay

Status: proposed catch-up architecture. The current delivery milestone is per-head
lineage within the existing complete proof; its qualification is recorded with that
commit. Version 2 segments, resumable replay and a prefix cache are not implemented
by this decision. The current 256-pair and byte limits remain unchanged.

## Context

[Native admission](2026-09-08-jj-native-admission.md) proves each packet back to the
original saved baseline or root. A long-lived source eventually exceeds one complete
packet's limits. More frequent [daemon observation](2026-09-08-jj-daemon-observation.md)
does not shorten that proof. A cursor, an observed operation ID or a stored checksum
alone cannot authorize a newer traversal boundary.

We will compose verified segments while preserving the original source, registration
receipt, reader profile and baseline identity/generation. No automatic rebaseline,
workspace adoption, attribution mode or second durable observer cursor is introduced.

## Per-head lineage: the current prerequisite

For each captured head, derive its exact reached original baseline IDs and root bit
from the already verified, parent-first graph. One bit per original anchor plus one
root bit fits the bounded set of at most 32 anchors. Combine parent bits once per
node; do not decode raw evidence again for every head or clone native payloads.
The aggregate closure remains the union of the per-head closures.

For example, prior heads A→B1 and X→B2 do not permit an A descendant to claim B2.
A root path reached only through X must likewise not appear in A's lineage. A later
segment can compose only the summaries of prior heads it actually reaches. These
summaries are derived results, never independent stored authority.

The public proof, captured-history and durable-admission results carry this metadata.
Debug admission JSON exposes `head_closures` beside existing aggregate fields;
receipt/cursor identities and native v1 bytes stay unchanged. This prerequisite does
not add prior-head terminals to today's collector or make histories over its current
limits admissible.

## Version 2 segments: planned first catch-up delivery

A distinct canonical domain/version will bind a segment to an exact prior admission
receipt, alongside the unchanged original scope, expectation, sampled heads and local
raw evidence. Generation zero closes directly to the original baseline/root; a
positive expected generation resolves its exact prior receipt, not whichever packet
is latest. A generation-index lookup is a candidate selection followed by complete
scope, identity, head-set and native-lineage validation.

A verified prior frontier, original baseline heads and root are separate terminal
categories. Their nonroot union has at most 64 IDs. Preserve the real BaselineReceipt;
do not fabricate one with newer heads. An older operation merely present in a packet
is not a terminal. Late branches that miss the prior frontier must still supply their
complete local ancestry to an authorized terminal. A missing or oversized side branch
refuses the entire request; this phase does not admit partial head closures.

Dependency generations must strictly decrease to a native-verified complete v1 packet
or generation-zero baseline. Resolve by indexed exact IDs, deduplicate packets selected
as latest, expected boundary, request or dependency within each snapshot, and reverify
canonical bytes and native evidence. Missing, conflicting, cyclic or wrong-scope links
refuse. Keep per-segment native, semantic, raw and encoded caps; add explicit cumulative
replay budgets. Bounded linked replay can pass 256 total operations across packets,
but eventually exhausts its own finite replay ceiling.

Writer and readers ship together. Preserve v1 canonical bytes and exact retries;
never equate v1 and v2 requests from generation/head equality alone. Freeze version
selection and unused-dependency rules before v2 tests, including the legacy request
reconstruction path. A historical generation-index candidate is only a hint until its
whole canonical request matches. Exact retry still precedes CAS and returns its old
receipt separately from the current cursor.

Results must distinguish local segment evidence from complete original-cutoff evidence.
Do not silently change `ordered_operations()` into an apparently complete list that
omits dependencies. Keep the public complete-history collector's existing contract.
Initial verified boundaries are rejoined under IMMEDIATE before writes, with current
registration, selected attachment and required dependency identities rechecked. Exact
post-write verification, owned result construction, retained source checks and the
sole commit retain their existing order; any failure before commit rolls back.

## Bounded restart replay and active cache: planned later delivery

Own one private existing-only WAL connection for the replay/cache lifetime. Start
pending, with no admission authority. A bounded backward anchor-discovery pass followed
by forward native verification can retain one packet and at most 32 head summaries.
Give each job explicit byte/work limits and release its read snapshot before yielding;
this is a new resumable lifecycle, not a reset of a one-shot ReadBudget. Total replay
time can grow with history, and repeated invalidation can starve completion.

Read `PRAGMA main.data_version` inside each new DEFERRED replay snapshot and compare
only on that same connection. Any observed change discards partial proof; opening a
replacement connection always starts empty. The value is connection-local, excludes
own commits and is not a content hash or durable revision. See SQLite's
[data_version contract](https://www.sqlite.org/pragma.html#pragma_data_version).

Before cached proof authorizes a write, acquire BEGIN IMMEDIATE, then compare the
epoch and validate exact schema, cursor, receipt, scope and selected target. A check
before reservation or in an old snapshot leaves a race. See SQLite's
[snapshot and writer isolation](https://www.sqlite.org/isolation.html). Epoch mismatch
invalidates proof; it never refreshes an active expected cursor. Startup/resume keep
their separate authorization to read current progress. Final promotion requires a
fresh epoch/scope check and fresh physical/policy validation.

Controlled own writes must preserve the cached prefix. Before cached packet/state
DML, reject applicable main or TEMP triggers under the same reservation and verify
the fixed schema. The private connection creates no TEMP triggers and permits only
the known append/state transition. A trigger can otherwise rewrite an old dependency
without changing this connection's data_version or the new packet's valid readback.
A `total_changes64` delta may supplement this guard but cannot replace it because
[change counting has exclusions](https://www.sqlite.org/c3ref/total_changes.html).
Preserve the existing uncached APIs' trigger/readback behavior separately.

Extend the cache only after confirmed commit. Own write errors, uncertain rollback or
commit, connection errors, observed external commits and changed schema invalidate it.
An idle cache cannot certify an arbitrary historical request without its actual replay.
No proof flag is persisted, and no read transaction pins WAL across scheduling waits.

Cache continuity assumes ordinary SQLite transactions/locking on the same live database.
Raw overwrites, selective bit corruption, live file replacement/restore, broken VFS
locking and finite-counter ABA are outside that continuity guarantee. It is narrower
than rereading every selected byte on every request. A supported `HAS_MOVED` refusal
can detect some pathname replacement without raw database opens, but is not a complete
identity protocol. Equal epoch values or paths are not rollback witnesses. SQLite
[documents live-file replacement and locking hazards](https://www.sqlite.org/howtocorrupt.html).

## Delivery and tests

| Increment | Required outcome before claiming that milestone |
| --- | --- |
| Per-head proof | Exact B1/B2/root composition, shared ancestors, deterministic head ordering and aggregate equivalence; unchanged v1 bytes and limits. |
| V2 writer plus readers | Beyond-256 total history split into bounded segments; finite dependency replay, corruption/late-branch refusal, version-specific retry, exact target/readback/CAS and byte-budget tests. |
| Restart/cache lifecycle | Many bounded replay jobs with no early activation; snapshots released at yields; external corruption and check-before-reservation races invalidate; own-trigger mutation refuses; commit uncertainty discards proof. |

Use independent native/CBOR fixtures and TestRepo tests before implementation. Freeze
exact v2 result/version/retry and per-job replay budgets as engineering contracts in
their respective increments. Keep Git/jj work outside Trace2 ingestion with no process
spawn per operation, file or dependency. Native attribution remains disabled.
