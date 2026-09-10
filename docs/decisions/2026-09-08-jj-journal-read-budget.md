# Shared jj journal read budget (ENG-415)

Status: accepted — maintained aggregate payload budget for observation lookups.

`lookup_observed_with_budget(source, ids, &mut ReadBudget)` lets one bounded
reader account for multiple journal lookups. `ReadBudget::new(bytes)`,
`remaining()` and `consumed()` expose a caller-owned allowance for encoded source
state and operation payload BLOBs selected by SQLite. Every call still reads its
state and requested evidence in one source-scoped transaction. A sequence of
calls is not a single transaction; the reader must compare their returned status
and perform its final generation/head check before capture.

The budget is charged before checksum, sequence, CBOR or envelope validation.
A corrupt result cannot refund previously selected bytes. A query suppresses a
payload that exceeds the remaining allowance and returns an error without
partial evidence. Empty or unknown lookups still charge existing source state;
a genuinely absent source can answer with zero payload allowance.

SQLite column affinity alone does not guarantee a binary payload or integer
sequence. The SQL guards require BLOB state/operation values before selecting
them and return only integer sequence values. Wrong-type bodies are rejected
without loading their contents. Checksums and identifier metadata remain bounded
separately. The counter measures encoded payload selection, not decoded raw
operation/view sizes, SQLite cache/page activity, physical disk I/O or elapsed
time.

## Limits and compatibility

The existing lookup delegates through the same implementation with a fresh
128 KiB state allowance plus 8 MiB operation allowance. These independent caps
remain enforced even if a caller supplies a larger aggregate budget:

- 128 requested full operation IDs, validated before deduplication.
- 128 KiB encoded source state and the existing encoded per-record limit.
- 8 MiB encoded operation payloads per call.
- 1 MiB combined decoded operation/view evidence per record.

Other state/record consumers reuse the same bounded loaders. The database schema,
capture transaction, generation checks, replay semantics and applied frontier
are unchanged. This budget neither verifies native ancestry nor turns an opaque
journal record into an admission boundary. The pure
[evidence verifier](2026-09-07-jj-native-evidence-verifier.md) remains a separate
per-record check.

## Verification

Nineteen TestRepo tests were installed before the API. RED failed on the missing
budget type and lookup method. GREEN passed all 19 cases: inclusive payload and
state boundaries, repeated lookups, exhaustion without partial results,
checksum/decode/sequence errors, large wrong-type metadata, oversized payloads,
independent caps, source isolation, orphaned state and requested-head gaps.
Expected charges come from actual stored BLOB lengths instead of private Rust
serialization details. Existing concurrent capture/lookup coverage continues
through the legacy method and shared transaction implementation.

```sh
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_journal_budget TEST_THREADS=2
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_observation_journal TEST_THREADS=2
```

A fresh build and Rust 1.93 all-target lint passed. Regression verification
passed 72 journal integration tests, two journal unit tests, 13 native evidence
tests, 12 source/storage policy checks and 33 fork workflow policy checks. The
journal subprocess helper is ignored in ordinary collection and explicitly
exercised by the recovery test. Formatting passed. No Git/jj subprocess or
Trace2 ingestion work is added by this increment.
