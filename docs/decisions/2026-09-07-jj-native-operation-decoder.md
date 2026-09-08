# Native jj operation decoder (ENG-415)

Status: accepted — maintained operation-file decoder contract; view decoding and observer integration remain pending.

`decode_operation(profile, expected_id, bytes)` in `src/operations/jj/operation/`
verifies one immutable jj operation against the explicit
`jj-simple-op-store/0.45.1` profile. It reconstructs jj's semantic domain and
checks its BLAKE2b-512 content address before returning owned evidence. Protobuf
field order and map entry order can change without changing that address;
parent and predecessor vector order remains significant.

The result includes the operation and view IDs, ordered parents, workspace name,
snapshot marker and optional commit-predecessor map. Missing workspace differs
from an explicitly empty workspace. Missing predecessor evidence differs from a
recorded empty map, and repeated predecessor edges remain intact. The referenced
view is not read or verified by this API. Successful decoding does not establish
integrated ancestry, authorize journal admission, or apply attribution.

## Compatibility boundary

The implementation follows the pinned jj 0.45.1
[operation domain](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/op_store.rs),
[protobuf schema](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/protos/simple_op_store.proto),
[normalization](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/simple_op_store.rs),
and [content hash encoding](https://github.com/jj-vcs/jj/blob/v0.45.1/core/src/content_hash.rs).
It accepts default or absent metadata/timestamps using jj's normalized defaults,
including signed timestamp and protobuf int32 cast semantics.

This is a strict subset of the general protobuf reader. Unknown fields, wrong
wire types, duplicate singular fields or map keys, duplicate operation parents,
invalid UTF-8, noncanonical boolean values, contradictory predecessor-presence
flags and malformed identities fail closed. Commit IDs must be SHA-1 sized;
operation and view IDs must be full 64-byte identities. Parentless legacy records
are rejected. The virtual all-zero operation has no file to decode, but remains
valid as a parent boundary. The profile is an explicit compatibility assumption,
not evidence of the writing binary's version.

## Resource bounds

| Per-operation limit | Maximum |
| --- | ---: |
| Raw protobuf bytes | 2 MiB |
| Operation parents | 32 |
| Predecessor map keys plus every vector edge | 4,096 |
| Metadata attributes | 256 |
| Aggregate UTF-8 metadata bytes, including attribute keys and values | 64 KiB |

The wire parser borrows bounded slices and checks lengths before splitting.
Counts are charged before collection insertion, and metadata bytes before UTF-8
validation. The semantic hash is streamed; no complete normalized byte buffer is
constructed. These are input and allocation bounds, not a wall-clock guarantee.
The journal has a separate, stricter 1 MiB operation-plus-view admission limit;
a decoded operation can still exceed journal admission policy.

The decoder makes no filesystem calls, subprocesses or Git object reads. It is
not connected to Trace2 ingestion, the daemon, or checkpoint processing. The only
new direct dependency is `blake2` 0.10.6; its digest feature adds `subtle` 2.6.1.
Existing locked package versions are unchanged. Python generates test vectors
only and is not a production runtime dependency.

## Verification

The integration tests were installed first and failed because the native API did
not exist. All 27 tests passed after implementation: 25 default tests plus two
explicit real-jj qualification tests. Synthetic vectors use an independent Python
`hashlib` semantic encoder and invented metadata. Tests exercise normalized field
ordering, optional/default distinctions, vector ordering, exact inclusive limits,
aggregate budgets, malformed nested fields and every truncated prefix of the
rich fixture.

The real-jj tests use isolated TestRepo fixtures in colocated and non-colocated
layouts. They decode operations created by snapshot, commit and describe, require
content hashes to match actual store filenames, and compare repository bytes
before and after decoding while leaving unsnapshotted work present. They run with
an explicitly selected jj 0.45.1 binary and do not install software or start the
git-ai daemon. This qualification was run on macOS; cross-platform real-jj
qualification remains pending.

```sh
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_operation TEST_THREADS=2
rtk proxy env GIT_AI_TEST_JJ_BINARY=/absolute/path/to/jj-0.45.1 gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_operation TEST_BINARY_ARGS=--include-ignored TEST_THREADS=2
```

Independent review found no actionable decoder or fixture issues. After integration,
the fresh build, Rust 1.93 all-target lint and format check passed, along with
53 journal tests, 15 context tests (including three real-jj cases), 12 source/storage
policy checks and all 33 fork workflow policy tests. The dependency lock was
validated with `cargo metadata --locked`; existing package versions did not change.

The [native view decoder](2026-09-07-jj-native-view-decoder.md) now verifies the
referenced view format independently. The [evidence verifier](2026-09-07-jj-native-evidence-verifier.md) joins both hashes
to journal envelopes. Bounded integrated-head traversal and validated journal
boundaries remain pending.
Operation decoding alone does not make native jj checkpoint attribution usable.
