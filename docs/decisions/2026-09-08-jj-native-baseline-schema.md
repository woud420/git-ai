# Journal schema for explicit native jj baselines (ENG-415)

Status: accepted — maintained schema migration; baseline installation is a separate API.

The observation database advances from schema version 1 to exact text version
2. Existing opaque operation, view, source-state and receipt bytes retain their
version-1 serialization. Two empty native tables add a separate namespace for
an immutable baseline record and its source progress pointer. Their composite
foreign key keeps the source and baseline identity together; neither table
references the opaque observation queue.

Initialization and upgrade reuse the existing SQLite IMMEDIATE transaction and
FULL-synchronous WAL connection. Creating both tables, changing the version,
validating the native columns/primary keys/foreign key and committing form one
transaction. The update must affect one row and the version must read back as
2, with exactly one version row. A failure, ignored update or trigger that silently rewrites the version rolls
back the entire upgrade. Existing version-2 databases are validated without
rewriting their records. Unknown versions, missing required tables, partial
native tables under version 1, duplicate version rows and malformed native
key/reference shapes reject
without repair or backfill.

A frozen SQL fixture contains the original five-table schema and independently
encoded opaque source/operation/view/receipt records. It first passed against
the version-1 reader before migration work. The tests preserve those bytes and
historical receipt behavior through upgrade and later captures. Separate tests
cover new initialization, complete version-2 reopen, native key/FK enforcement,
rollback faults and two concurrent initializers. Logical schema/record rollback
is tested; this does not simulate power failure or require byte-identical WAL
files.

This migration neither verifies native history nor enables source registration,
baseline installation, observation, checkpoint attribution or application. It
adds no Git/jj subprocesses or work to Trace2 ingestion.

Final verification: all 86 journal integration tests passed, including the
fourteen migration cases; the two journal unit tests, sixteen default baseline
preparation tests, twelve source/storage policy tests and 33 fork workflow
policy tests also passed (149 top-level tests). The journal recovery test
explicitly drives its ignored subprocess helper. The unchanged real-jj baseline
preparation case remains separately qualified by its prior run. Fresh build,
Rust 1.93 all-target lint and final formatting passed. The duplicate-version
regression first failed against the initial v2 implementation before the bounded
cardinality fix. Logs are retained as `git-ai-jj-schema-v2-*`.
