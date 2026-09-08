# Bounded inspection of native jj registration records (ENG-415)

Status: accepted — record inspection; complete registration and onboarding remain separate.

## Scope

The journal exposes `read_native_registration_record` for one source and
`read_native_workspace_record` for one exact source/workspace name. Each returns
the original canonical record bytes or an absent-row result under a caller-owned
`ReadBudget`. The returned bytes describe only the selected record. They do not
prove a complete registration, native baseline, filesystem identity or current
workspace readiness. Individually valid orphan records remain inspectable.
An absent row does not authorize initialization or distinguish first use from
erased history.

This increment supplies private record codecs through maintained read APIs.
It adds no record writer, source seal, attachment mutation, automatic repair,
capture conversion or complete source join. Existing native and opaque record
formats remain unchanged. No Git/jj process or Trace2 ingestion work is added.

## Wire contract

Source and workspace records have independent version-1 domains and exact
ordered CBOR maps. Source records bind source/profile/baseline generation 1,
seal bytes and digest, eight sampled directory identities, three backend byte
samples, and the original workspace name, attachment ID and record digest.
Workspace records bind source/profile/baseline/seal, workspace name and
attachment ID, raw locator, four sampled directory identities and a historical
checkout receipt. Directory identities are unsigned device/inode pairs.
Platform values are `linux` and `macos`, inspectable on either platform.

All structs reject unknown fields. Arrays have exact lengths, enums have fixed
spellings, and canonical re-encoding must equal the stored bytes. New byte fields
use CBOR byte strings; prior baseline integer-array encodings remain exact.
The shared codec checks checksum, framing, declared lengths and nesting before
deserialization. The owned byte-buffer visitor accepts values larger than
Ciborium's borrowed-byte scratch buffer without cloning accepted buffers.

Each complete record is capped at 128 KiB. Seal bytes are nonempty and at most
1 KiB; backend samples, workspace names and checkout bytes are nonempty and at
most 16 KiB each; the raw locator is at most 64 KiB, starts with `/` and contains
no NUL. This rooted-byte check does not resolve or certify a filesystem path.
Names preserve exact UTF-8 without case folding or normalization. IDs use their
existing lowercase 64/128-hex formats; the checkout operation cannot be root.
The seal digest must match its bytes. Seal, backend and checkout contents remain
opaque; their native meaning and cross-record relationships need later checks.

The source-root and workspace-root guards hash their separate fixed domain,
platform and root device/inode pair. The locator guard hashes its own domain,
platform and exact path bytes. These keys are negative conflict/lookup hints,
never positive source identity. Inspection recomputes the selected row's keys.

## Reads and limits

Each read validates the request, opens one SQLite read transaction, and checks
at most two matching metadata rows. Zero returns absent; duplicates reject
before any payload is selected. Exactly one permits a single payload select.
Both stages use explicit BINARY comparisons and the declared primary key.
Related native state, original workspace and sibling records are not scanned.

SQL suppresses wrong-type or oversized BLOBs and type/byte-length gates scalar
columns. A complete selected BLOB is charged before checksum, scalar or decode
validation; failures retain that charge. Repeated calls charge again. One call
selects at most one 128 KiB record; the budget measures selected encoded bytes,
not SQLite page I/O or peak allocator usage. Decoded and re-encoded forms may
temporarily coexist, and field visitors can run after Ciborium allocates a field
within the whole-record cap.

## Verification

The public suite first failed only on the two missing APIs; the private suite
failed on the missing query constants and decoder. The implementation then
passed all 18 TestRepo cases and four private cases. All 15 canonical and 40
malformed independent vectors pass through the public readers. The 59 generated
outputs reproduce byte-for-byte, with separate review of canonical hashes,
negative-key encoding and fixture setup. Valid maxima are 67,492 source bytes
and 99,328 workspace bytes; a valid exact-128-KiB record is unreachable under
these field limits.

The broader default jj integration suite passed 302 cases, with 14 explicit or
child lanes ignored by that command. Build passed. Independent compatibility
and resource reviews cleared the new codecs and read paths. Final journal,
policy, lint and formatting checks are recorded with the published increment.

Private query-plan tests inspect the actual production SQL under canonical
schema and a workspace-name column whose NOCASE default differs from its
explicit BINARY primary key. Public behavior tests separately distinguish
case and Unicode spellings. Logical schema/row preservation is checked around
reads; it does not imply byte-identical WAL files or a power-loss simulation.

Complete registration later requires a checked source/original-workspace/native
join in one snapshot, fresh source-seal and capture evidence, and native baseline
reverification. These record readers cannot substitute for that authority.
