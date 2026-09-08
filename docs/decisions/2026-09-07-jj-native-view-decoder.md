# Native jj view decoder (ENG-415)

Status: accepted — maintained immutable view decoder contract; attribution integration remains pending.

`decode_view(profile, expected_id, bytes)` verifies one immutable view under the
explicit `jj-simple-op-store/0.45.1` profile. It returns the verified view ID,
sorted commit heads, workspace-to-commit map and semantic reference count. The
workspace map describes what should be checked out; completed checkout evidence
remains separate. The decoder does not read files, traverse operation ancestry,
query Git objects, admit a journal batch, or enable attribution.

## Semantic contract

The BLAKE2b-512 hash covers all seven normalized domain collections, even those
not exposed in the returned summary: commit heads, local bookmarks, local tags,
remote views, Git refs, per-workspace Git heads and working-copy commits. Heads
are sorted as a set and maps by key. Remote views hash bookmarks before tags;
remote references retain their state. Conflict terms preserve the exact ordered
add/remove/add sequence, optional values and repetitions without simplification.

The implementation follows pinned jj 0.45.1
[domain declarations](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/op_store.rs#L249),
[protobuf definitions](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/protos/simple_op_store.proto),
[normalization and compatibility](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/simple_op_store.rs#L608),
and [content hash encoding](https://github.com/jj-vcs/jj/blob/v0.45.1/core/src/content_hash.rs).

Current jj still writes legacy remote bookmark and default Git HEAD mirrors.
Their targets, states and names must agree with the authoritative representations.
A missing default Git HEAD mirror is accepted; a supplied mirror must match the
corresponding current entry. Tag-only and empty remote views remain significant.
Exactly absent local bookmark targets are dropped during normalization; absent
targets in the other reference maps remain. Raw duplicate names are rejected
before any entry can be discarded.

Unknown/reserved fields, wrong wire types, duplicate scalar/map fields, malformed
UTF-8, invalid conflict arity and noncanonical remote states fail closed. Migration
flag 12 must be true. Legacy working-copy/Git-head bytes, old RefTarget forms and
GitRef fallback representations are unsupported. Commit IDs must have SHA-1 size;
view IDs must be full lowercase 128-character identities. The all-zero view is a
virtual sentinel without a file. An all-zero SHA-1 commit reference remains valid.
The profile expresses a compatibility assumption, not proof of the writing binary.

## Independent resource bounds

| Per-view budget | Maximum |
| --- | ---: |
| Raw protobuf bytes | 2 MiB |
| Normalized semantic commit references | 4,096 |
| Aggregate UTF-8 name bytes on the wire | 64 KiB |
| Repeated/map message entries plus explicit ref terms | 8,192 |

Semantic references count heads, working-copy values and every present term in
retained references. Repetitions count; compatibility mirrors do not count twice.
The wire-entry and name budgets include mirrors, absent terms and entries later
discarded by normalization. Head byte fields consume the semantic-reference
budget; singular wrappers/scalars and a synthetic absent term for a missing target
do not consume wire entries. This keeps absent-term and mirror growth bounded
independently of the number of commits referenced.

Wire fields use borrowed slices. Length, entry and byte limits are checked before
related allocation; semantic hashes are streamed. These limits bound parsing and
allocation, not elapsed filesystem latency. The caller still owns bounded file
reads, integrated-head selection and batch-level budgets. The journal retains its
separate 1 MiB combined operation/view admission limit.

## Verification contract

The tests were installed before the native export. The initial run failed solely
because `git_ai::operations::jj::view` did not exist. The test package contains
34 default tests and two explicit real-jj qualification lanes. Its 24 independent
synthetic vectors cover every domain collection and boundary hashes; a separate
research decoder verified all vectors, and independent review checked them against
the pinned jj sources. Python generates test fixtures only.

Both real-jj TestRepo layouts create tags and tracked refs in a local bare remote,
a linked workspace, and two concurrent bookmark moves from the same immutable
operation. After integration, the tests require an actual bookmark conflict and
a two-parent operation, then verify all operation-to-view hash joins while both
workspaces contain unsnapshotted files. Repository manifests must remain byte
identical across decoding. These fixtures use an explicitly selected jj 0.45.1
binary and no network, daemon or machine-wide installation.

```sh
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_view TEST_THREADS=2
rtk proxy env GIT_AI_TEST_JJ_BINARY=/absolute/path/to/jj-0.45.1 gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_view TEST_BINARY_ARGS=--include-ignored TEST_THREADS=2
```

All 36 native tests passed on macOS, including both explicit real-jj 0.45.1 lanes.
Independent review found no actionable issue in the hash semantics, mirror checks,
allocation bounds or shared operation helper extraction. The wire decoder and hash
primitives are shared with operation decoding; no dependency was added.

The fresh build, Rust 1.93 all-target lint and format check passed, along with
27 operation tests, 53 journal tests, 15 context tests and 45 source/storage/fork
policy checks. Lint first found fixture-only clone/concatenation warnings; these
were fixed in the test builder/generator without changing any raw bytes or hashes,
and all 36 view tests passed again.

The [checkout decoder](2026-09-07-jj-native-checkout-decoder.md) now decodes
completed workspace context. Bounded integrated-head traversal, consistent
checkout sampling and journal admission remain
subsequent increments. Verifying an individual persisted record cannot certify
opaque ancestors behind it; efficient resume requires an explicit validated
admission boundary tied to journal generation.
