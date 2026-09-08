# Native jj completed-checkout context (ENG-415)

Status: accepted — maintained checkout decoder contract; consistent sampling and admission remain pending.

`decode_checkout(profile, bytes)` decodes the workspace name and operation ID in
jj's mutable checkout record. Under the explicit `jj-simple-op-store/0.45.1`
profile, field 2 is a nonroot 64-byte operation ID and field 3 is a nonempty UTF-8
workspace name. Names are preserved without trimming or normalization. Raw input
is capped at 16 KiB before parsing or copying. Shared bounded wire helpers reject
reserved/unknown fields, wrong wire types, duplicates and truncated/overflowing
lengths. No new dependency is needed.

Checkout has no semantic content checksum in this format. Decoding its two fields
does not verify the referenced operation, completed filesystem/tree consistency,
integrated ancestry, journal admission or attribution. The caller must perform
bounded file reads and recheck these mutable bytes when sampling a workspace.
Joining the checkout's own operation to its immutable view supplies the recorded
workspace commit. The latest view instead describes what should be checked out;
using it for a stale workspace would select the wrong base.

Pinned jj 0.45.1 defines the fields in
[local_working_copy.proto](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/protos/local_working_copy.proto#L69).
[CheckoutState loading/saving](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/local_working_copy.rs#L2569)
uses a default workspace when the stored name is empty; this strict decoder rejects
that legacy fallback. [Workspace completion](https://github.com/jj-vcs/jj/blob/v0.45.1/lib/src/local_working_copy.rs#L2923)
updates checkout after tree state. The profile expresses a compatibility assumption,
not a detector for the binary that last wrote the record.

## Verification

Twelve default tests and one explicit real-jj TestRepo lane were installed before
the native export. The initial compile failed on missing checkout/evidence APIs
and no other error. Synthetic tests cover exact profile and field validation,
operation IDs, UTF-8 preservation, incomplete prefixes and the inclusive 16 KiB
boundary for ASCII and Unicode names.

The real fixture creates a linked workspace, then rewrites its working-copy commit
from the primary workspace. Its checkout remains byte-identical while the latest
view changes. With both workspaces dirty, the test joins checkout through its own
operation/view and requires that commit to differ from the latest view's value.
The complete TestRepo manifest must remain unchanged. The fixture uses an explicit
jj 0.45.1 binary, without a network remote or machine-wide installation.
The existing bounded fixture reader is shared with the real view tests.

```sh
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_checkout TEST_THREADS=2
rtk proxy env GIT_AI_TEST_JJ_BINARY=/absolute/path/to/jj-0.45.1 gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_checkout TEST_BINARY_ARGS=--include-ignored TEST_THREADS=2
```

All 13 checkout tests passed, including the explicit pinned real-jj lane.
Regression gates passed: 36 view, 27 operation, 13 evidence, 15 context, 53 journal,
12 source/storage policy and 33 fork workflow policy tests. The journal
subprocess-only helper remains ignored in ordinary collection and is exercised
by its recovery test. A fresh build, Rust 1.93 all-target lint and formatting
passed.

Descriptor-relative metadata reads, consistent sampling, validated ancestry
boundaries and checkpoint ordering remain subsequent increments; this decoder
does not change the existing Git workflow.
