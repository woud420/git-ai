# Durable jj observation journal: first implementation increment

Status: accepted — maintained journal contract; native reader and attribution integration remain pending.

This ENG-415 increment persists captured evidence. It does not run an observer,
register a workspace, invoke jj/Git, apply attribution, or alter the daemon's
Trace2 path. The [reader proof](2026-09-07-jj-reader-proof.md) remains research
code; the native reader and daemon integration are subsequent increments.

## Contract

The journal has an explicit database path and an independent schema. Every batch
names its source store, wire version, reader profile, expected generation, expected
observed heads, sampled integrated heads, and operation evidence. The profile
records the decoder compatibility assumption; it is not a writer-version detector.
Raw operation/view bytes are opaque at this layer. A future native reader must
verify jj content addresses and associate extracted fields with those bytes before
any persisted evidence can drive attribution.

Capture checks IDs, duplicate entries, collection/byte limits, immutable evidence
collisions and the declared DAG. Every included new operation must be reachable
from the declared captured heads. A parent boundary may be any previously stored
operation in the same source, which supports late branches from older ancestors.
The journal cannot prove that the caller sampled real integrated heads; that is
the reader boundary's responsibility.

An immediate SQLite transaction commits operation records, the batch receipt,
and observed generation/head state together. An identical historical retry is
acknowledged without rewinding newer progress. Generation checks distinguish a
fresh repeated head transition from a retry, including `A -> B -> A` changes.
The journal retains raw redundant/concurrent head sets. A second writer with stale
generation or expected heads cannot overwrite the first writer's observation.

Checksummed, versioned records are verified before decoding or reuse as a known
boundary. Historical receipts also require the packet's heads and explicit parents
to remain present. Pending reads compare a bounded ordered prefix with the
checksummed state count and sequence numbers, rejecting missing rows within that
prefix. Pending reads do not scan or audit the entire stored history.

Admission caps each operation plus view at 1 MiB of raw evidence, a batch at
256 operations and 8 MiB of raw evidence, heads at 128, and parent fan-in at 64.
The encoded packet and the aggregate encoded stored-operation envelopes must each
fit 8 MiB. Stored records are individually capped at 2 MiB plus 64 KiB. Capture
reads incoming-ID records and external boundary/view representatives under
separate 8 MiB budgets, at most 16 MiB combined. These packet-defined partitions
remain stable across publication, so a successfully accepted packet fits the same
read budgets on retry. Pending reads have their own 8 MiB budget and a caller
limit of 1–128 records. CBOR preflight rejects excessive nesting and impossible
collection lengths before deserialization.
Schema initialization rejects malformed, missing-on-existing, or unsupported
versions. The journal requests SQLite `synchronous=FULL`; successful capture means
SQLite acknowledged the transaction under that policy, not a claim that every
filesystem or storage device honors power-loss guarantees.

Observed state is separate from attribution application. Applied heads remain
empty and there is no application-acknowledgment API in this increment. Later
ordering/replay work must make authorship side effects durable before advancing
application progress. This prevents a successful capture from being mistaken for
completed attribution.

## Verification scope

TestRepo creates isolated repositories and explicit test-home databases without
starting a daemon. Synthetic raw bytes intentionally test journal integrity;
they are not disguised as valid jj protobuf objects. Tests cover reopen and
process-exit recovery, rollback after a mid-transaction SQL failure, historical
retry, generation races, late divergence, redundant heads, gaps/cycles, source
isolation, identity collisions, corrupt boundaries and bounded pending reads.

The process-exit fixture bypasses connection destruction after capture and then
reopens the database. It verifies recovery from a committed WAL without cleanup;
it does not simulate sudden power loss. Real jj semantic qualification belongs
to the reader tests, and native line attribution is still pending.

## Implementation and evidence

The domain envelope is in `src/model/jj_observation.rs`; storage, graph validation,
and bounded codec are in `src/model/repository/jj_observation_journal/`. The journal
reuses the repository SQLite connection helper and `PersistenceError`. It adds no
dependencies, subprocesses, Git object reads, or daemon ingestion work.

The TestRepo suite was introduced before production exports. Its first run failed
on the absent API. The first implementation passed 33 behavioral tests. Review
then added four regressions and reproduced actual failures before applying fixes:
missing heads on an empty historical receipt, a missing historical parent behind
the current frontier, missing pending rows, and an accepted large packet that
could not be retried within the previous combined read budget. After the fixes,
all 37 behavioral tests pass. One subprocess-only helper is ignored by the runner
and explicitly executed by the process-exit recovery test.

Repeat the journal checks through the repository Make interface:

```sh
rtk gmake build
rtk gmake test CARGO_TEST_ARGS=--lib TEST_FILTER=jj_observation_journal TEST_THREADS=2
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=jj_observation_journal TEST_THREADS=2
```

Initial increment checks passed: 37 journal integration tests, two journal unit tests,
12 context diagnostic tests plus three explicit real-jj qualification tests, and
12 repository policy checks. The policy run first caught direct SQLite opens in
the fault-injection tests; those now use `open_with_memory_limits`, and both the
policy suite and all journal tests passed again. The fresh binary build, Rust 1.93
all-target Clippy, and format check passed. Use the repository's Rust 1.93
toolchain for `gmake lint` and `gmake format-check`.
The full Git attribution suite and Windows cross-check are outside this isolated
storage increment's verification scope. The real-jj context and reader suites
remain separate from the synthetic evidence journal tests.

## Bounded observed-evidence lookup

The follow-up `lookup_observed(source, operation_ids)` API supplies the reader's
resume boundary. It returns source progress and only the requested stored records
from one SQLite read transaction. A reader can retrieve an old ancestor behind
the current head set or beyond the bounded pending prefix without scanning history.
The native decoder must still verify each returned operation's jj content address
before treating it as a traversal boundary; a journal checksum establishes only
capture integrity.

Inputs are limited to 128 unique full operation IDs. Root, duplicate and malformed
IDs are rejected. The existing record decoder enforces the same 8 MiB aggregate
read budget; an oversized or corrupt request fails without a partial result.
Ordinary unknown IDs are omitted. A requested ID named by the stored observed
head set must exist. Returned record sequences must fall within the stored pending
count. Empty lookups still validate state, and source-indexed existence queries
reject orphan operation, receipt or view rows when the source state is missing.
These checks do not audit unrequested history or assert that captured heads still
match the live jj repository.

Sixteen additional TestRepo tests were added before implementation, including
concurrent captures, reopening an old ancestor, source isolation, count/byte
limits, corrupt state, orphan rows, and the distinction between unknown IDs and
missing observed heads. All 53 journal integration tests then passed, alongside
12 source/storage policy checks and Rust 1.93 all-target lint. No dependencies,
schema changes, CLI behavior, or daemon work were added.

## Next increment

Port the qualified immutable reader to Rust and use `lookup_observed` for its
bounded durable membership queries. Qualify the adapter against real jj operations and
missing-history cases before connecting an observer or checkpoint ordering.
Workspace registration, notifications, native capture and attribution application
remain unimplemented; ENG-415 stays in progress.
