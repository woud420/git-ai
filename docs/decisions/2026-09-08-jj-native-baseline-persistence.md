# Durable current-state jj baseline

Status: accepted — maintained explicit persistence API; source registration remains separate.

The selected baseline remains a cutoff: operation and view bytes verify, while
ancestry before those anchors remains unverified. Persistence must not turn an
anchor into a queued operation or fabricate observation/application progress.

## Decision

Store the first native baseline in the two additive schema-v2 tables,
`jj_native_baselines` and `jj_native_sources`. Opaque evidence record versions
remain 1, and existing opaque tables and capture behavior remain unchanged.
There is no opaque-source foreign key, data backfill, CLI call, observer, reset,
or attribution application in this increment.

The operations coordinator accepts `PreparedCurrentStateBaseline`, which already
proved exact envelope/native joins. Repository methods remain crate-private and
decoder-independent. They persist a structural record only. Reopening calls the
existing pure native baseline preparation again before returning an immutable,
owned durable result. No stored verification flag or deserializable public proof
can bypass that verification.

First install accepts expected native generation 0 and records generation 1.
An identical generation-0 request returns its existing receipt even though the
stored native generation is 1. A changed request conflicts; there is no overwrite
or generation-2 protocol. Opaque captures have independent progress and cannot
widen this cutoff.

## Identity and atomicity

The immutable record is also the receipt. Its SHA-256 identity covers canonical
CBOR including record version 1, domain
`git-ai/jj/current-state-baseline/install/v1`, caller source ID, exact reader
profile, `current_state` mode, expected native generation, captured heads, and
original evidence. Heads and anchors are sorted by identity; ordered parents and
both raw byte strings remain unchanged. Thus collection reordering is an
identical request, while equivalent but different protobuf bytes are not.

Install pre-encodes under fixed caps, starts one IMMEDIATE transaction, validates
existing native state and its one source-local receipt, and only then chooses
idempotent retry or the absent-state first install. Both inserts must affect
exactly one row. Before commit, a fresh bounded read inside the same transaction
must reproduce the exact request and state. This catches successful inserts
whose triggers remove or alter the stored result. A failure rolls back both.
SQLite uses the journal's existing
FULL synchronous, WAL, foreign-key, cache, and bounded busy-timeout policies.
Commit acknowledgment is not a universal power-loss guarantee.

A missing state with a retained receipt is a gap. A missing active receipt,
unexpected extra source-local receipt, scalar/payload identity disagreement, or
corruption is an error. Neither reinstall nor opaque-row fallback repairs these
conditions. Read-side source state and receipt checks share one transaction.

## Bounded records and verification

The complete encoded baseline is capped at 8 MiB and native state at 128 KiB.
The existing raw caps remain separate: 32 heads/anchors, 8 MiB combined raw
evidence, and 1 MiB per operation/view pair. A valid pure prepared baseline may
exceed the encoded storage cap; storage then rejects it without progress.

Requests canonicalize bounded references without cloning evidence. SQL checks
types and lengths before selecting a complete BLOB under the smaller of its
record cap and remaining caller budget. Identity/checksum scalar columns are
also type/length gated. Every selected BLOB is charged before subsequent metadata,
checksum, decoding, digest, or native failures; charges are never refunded.
The budget measures encoded BLOBs materialized by these queries, not SQLite's
internal/page I/O or all decoder allocations.

The shared codec validates framing, declared lengths, depth, and checksum.
Bounded sequence visitors reject excessive collection hints before allocation or
element deserialization. Stored record/state contracts, canonical serialization,
request digest, and cross-record identities must agree. The coordinator then
re-verifies native operation/view hashes and exact envelopes on every reopen.

## Limits of this result

`source_id` is a caller-supplied namespace, not a physical store certificate.
The durable result does not prove current heads, continuous source identity,
ancestry through an anchor, future admission, workspace readiness, or attribution.
No production caller is added before filesystem capture and registration have
their own qualified contracts.

Complete deletion of all native records is indistinguishable from a new source
inside this database alone. Automatic bootstrap therefore needs a separate
registration/epoch policy; this storage API does not solve rollback, copy/restore,
inode reuse, or malicious erasure. A later reset or historical-verification mode
must make the discontinuity explicit and preserve appropriate receipts.

## Verification

The integration package includes 27 default TestRepo cases and one ignored pinned
real-jj lane. It covers canonical/raw-sensitive retries, source isolation, opaque
independence, concurrent writers, atomic, ignored-insert and after-insert readback faults, missing/extra
records, malformed types/limits, budgets, and native corruption after storage
checksums/digests have been repaired. The real lane leaves prior parents unread
and checks unchanged source/workspace files after install/reopen/retry.

All 28 persistence cases passed, including the explicit pinned real-jj lane.
The complete increment passed 180 top-level tests: 45 baseline preparation and
persistence cases, 86 journal cases, four journal unit tests (including the two
sequence-hint checks), twelve source/storage policies and 33 fork workflow
policies. The ignored journal subprocess helper is exercised by its recovery
test. Fresh build, Rust 1.93 all-target lint and final formatting passed.

TDD began with the missing persistence API. The after-insert regression then
failed against the initial implementation before transactional readback was
added. Independent review cleared the final change. Local runtime qualification
is on macOS; new-head Linux and Windows execution remains a CI gate. Logs are
retained as `git-ai-jj-baseline-persistence-*`.
