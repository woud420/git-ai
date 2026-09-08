# Native jj evidence envelope verification (ENG-415)

Status: accepted — maintained single-record verification contract; ancestry and admission remain pending.

`verify_evidence(profile, evidence)` joins an opaque `JjOperationEvidence` envelope
to the native operation and view decoders. It reuses the envelope's identity,
parent and combined 1 MiB byte validation, verifies both semantic content hashes,
and requires exact agreement between the decoded operation and the envelope's
operation ID, view ID and ordered parents. A valid view belonging to another
operation is rejected even when its own hash is correct.

A successful `VerifiedJjEvidence` borrows the original evidence and owns its decoded
summaries. Its fields are private and its accessors return immutable references;
it has no public proof constructor. The verifier preserves original wire bytes,
including semantically equivalent encodings, and does not copy or rewrite the
journal record. Underlying validation/decoder errors retain their context.

This verifies one envelope and its operation-to-view join. It does not establish
parent existence, integrated ancestry, current checkout, journal admission or
attribution. In particular, a verified persisted boundary cannot certify opaque
ancestors behind it. Missing predecessor evidence remains distinguishable from a
recorded empty map and is left for later reader policy. No Git/jj commands,
filesystem reads, journal writes or daemon work occur in this verifier.

The journal intentionally accepts bounded opaque evidence. Its checksum can be
valid even when declared parents are false or its view bytes belong to another
operation. The new verifier is the explicit boundary for detecting those semantic
mismatches; it does not change the journal schema or make old capture receipts
native ancestry certificates.

## Verification

The 13 TestRepo/pure tests were installed before production exports. The initial
compile failed on the missing evidence and checkout APIs, with no other errors.
Six independently generated operation/view fixture pairs cover a root, branches,
merge, unknown ancestry and unrecorded predecessors. Their hashes and joins were
checked with the separate research decoder and independently reviewed. Extending
the operation oracle with an optional view ID leaves every existing vector intact.

Tests capture and reopen real isolated journal databases, then verify bounded
lookup results. They cover wrong envelope parents, reversed parent order, swapped
independently valid views, native hash failures despite journal checksums, malformed
bytes, profile/envelope limits, equivalent encodings and borrowing the exact source
evidence. Verification must leave journal status, pending evidence and repository
bytes unchanged. Successful isolated joins do not admit unknown ancestors.

```sh
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_evidence TEST_THREADS=2
```

All 13 verifier tests passed after implementation. The fresh build, Rust 1.93
all-target lint and format check passed, as did 27 operation tests, 36 view tests,
53 journal tests and 45 source/storage/fork policy checks. Independent review found
no actionable issue. No dependency or journal schema change was added.

Checkout decoding and an explicit validated admission contract remain separate
increments.
