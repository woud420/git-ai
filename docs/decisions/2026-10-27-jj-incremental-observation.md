# Incremental native jj observation (ENG-415)

Status: accepted — linked segments and connection-owned bounded replay; native attribution remains disabled.

## Scope and compatibility

The experimental shared-daemon observer now consumes native evidence in bounded
segments. Each segment still has the existing 256-operation/view-pair, raw-byte,
semantic-reference and encoded-byte ceilings. Its exact original-cutoff lineage
composes through an independently verified previous admission. A long-lived source
can advance beyond 256 total operations without moving its original baseline.

Explicit `admit_registered_history`, `reconcile_registered_history` and the finite
foreground observer retain their complete v1 packet policy, canonical bytes,
historical retries and selected-byte accounting. The preserved characterization
tests from local commit `e51bdfd8e` cover repeated/alternating admissions beyond
eight generations and exact-budget rollback. That original commit is retained.

The first changed daemon observation is a complete v1 packet. Later observations
use `git-ai/jj/native-admission/segment/v2`, recording an exact
`previous_admission_id` in addition to the unchanged registration/profile/original
baseline and expected generation/head set. Existing admission tables and the v1
state format remain unchanged. Readers reject unknown versions, mismatched scope,
missing dependencies, wrong generation/head links and noncanonical records.
Older binaries reject v2 rather than interpreting it as a complete v1 history.

`ObservedJjSegment::local_operations()` describes only a segment. Its per-head
closures describe the exact original-cutoff reachability after composition.
The existing complete-history read API reconstructs and verifies small v2
histories while retaining its 256-operation and finite read-budget limits. It
explicitly directs larger reads to the resumable segment API instead of returning
a misleading incomplete `ordered_operations()` result.

## Bounded restart replay

`JjObservationSession` owns one journal connection. A replacement session starts
without cached admission authority. Each job reads at most one dependency packet
beside the bounded latest/registration snapshot. Backward discovery locates a
native-verified complete v1 anchor; forward replay verifies exact receipt links,
native hashes, deterministic parent-first order and per-head original lineage.
Only a completed prefix authorizes a new segment. This retains one packet and a
bounded head summary rather than a list of all historical packets or operations.
Snapshots are released between jobs; total recovery time can grow with history.

Pure persisted-evidence replay performs no Git/jj spawn or Git object lookup per
dependency. Full live source, workspace and policy validation occurs when opening
a session and before a ready read/admission, independently of replay length.
The separate applied frontier remains empty; none of these observations assigns
credit or delivers shared Git/jj checkpoint ordering.

Daemon status exposes `runtime: replaying` with a null `session_cursor` until the
saved frontier has been verified. A completed observation exposes its verified
cursor. Replay jobs wait 250 ms between attempts; active observation keeps the
existing five-second cadence. Each job retains a cooperative five-second deadline
and a 96 MiB selected-record budget; opening retains the 32 MiB budget. These are
work/read ceilings, not hard syscall deadlines or total RSS claims.

## Cache continuity and admission

Read `PRAGMA main.data_version` inside each fresh snapshot on the same connection.
An observed external commit discards partial/cached proof. A changed admission
cursor or selected target stops an active session rather than adopting it. Only a
new session, daemon startup or explicit resume reads fresh source-wide progress.

Before cached proof can authorize a write, acquire `BEGIN IMMEDIATE`, then check
the epoch, fixed schema, exact current receipt/cursor and original scope. Refuse
triggers on native admission/registration/baseline tables and all TEMP triggers:
own commits do not change `data_version`, so an own-write trigger could otherwise
alter an already verified dependency. Retain transactional exact readback and the
final held-source recheck before the sole commit. Promote cache state only after
confirmed commit; any failure invalidates it.

The journal transaction itself checks the schema before selecting packets and
requires each fresh v2 write's previous receipt to equal the current receipt.
Generation and head equality alone are insufficient. Exact historical retries
remain separate from this fresh-write check and keep the later cursor intact.

SQLite's `SQLITE_FCNTL_HAS_MOVED` rejects ordinary database rename/unlink without
opening a raw database descriptor. Continuity still assumes ordinary SQLite
transactions and locking on the same live database. Raw byte replacement/rollback,
broken VFS locking and finite-counter ABA are outside that guarantee. An equal
epoch or pathname is not an independent rollback witness.

## Failure and support boundaries

Missing/corrupt recorded dependencies, unsupported versions, ambiguous lineage,
live-source changes and budget overflow fail closed without advancing progress.
Restoring required evidence and explicitly resuming replays it again. Native
operation files already captured in verified packets may be garbage-collected;
restart replay uses their retained bytes. It does not recreate missing native
files or invent unobserved history.

One previously unseen branch larger than a segment budget remains unsupported in
that attempt; the observer does not admit partial head closures, automatically
rebaseline, raise the per-segment caps or scan arbitrary history in one job.
This is distinct from the removed permanent original-cutoff limit: successive
bounded observations and arbitrarily many linked generations can now advance and
restart. Platform/performance/packaging/downgrade rollout qualification remains P10.

## Verification contract

Regressions cover more than 256 total native operations in two layouts, replay
after native-history GC, more than 64 linked generations, no held snapshots at
yields, missing dependency restoration, external progress, short-budget rollback,
own-trigger refusal and database movement. Dedicated-daemon tests additionally
cover segmented catch-up/restart while preserving original registration rows.
Review regressions also exercise rehashed invalid links, expected heads and native
bytes; interruption and corruption of an old dependency with an unchanged cursor;
replacement by an identical database; and schema drift on an open writer before
any packet bytes are read. Restoring the evidence and reopening must recover the
same original baseline without advancing the applied frontier.
The CLI reply regression first rejected `replaying`; it now accepts that state
only with a saved target and without a purported verified cursor.

Use the repository's `gmake test TEST_FILTER=jj_`, the explicit pinned real-jj lane,
unit tests, lint, formatting and source-length/documentation policies. Test fixtures
and daemons stay isolated; no installed git-ai runtime or real user repository is
an input. The shared Git Trace2 ingestion path is unchanged.

The final local candidate was checked on 2026-10-27 with Rust 1.93.0:
`gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_` passed
500 tests (32 explicitly ignored real-jj/child entry points); the pinned
`jj 0.45.1` ignored lane passed 32 entries. The lane includes real colocated and
noncolocated reader, observer and operation-family tests. `gmake test
CARGO_TEST_ARGS='--lib'` passed 3,082 tests (three ignored), and the integration
`policy` filter passed 31. `gmake lint` and `gmake fmt` passed. Rust 1.97.1
`gmake check-windows` passed. The updated Mermaid diagram was rendered and
visually checked. These are local checks, not CI, publication or rollout evidence.
