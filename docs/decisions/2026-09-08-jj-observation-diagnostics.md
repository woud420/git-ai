# Read-only diagnostics for native jj observations

Status: accepted — implemented and locally qualified on macOS for the pinned jj profile.

Refs: [ENG-415](https://linear.app/polarcoordinates/issue/ENG-415),
[native admission](2026-09-08-jj-native-admission.md),
[shared workflow proposal](2026-09-07-jj-support-design.md).

## Decision

Expose two experimental commands through the existing standalone binary:

```text
git-ai debug jj status --journal PATH --json
git-ai debug jj receipt --journal PATH --source ID --admission ID --json
```

The journal path is explicit. These diagnostics inspect a previously initialized
journal; they provide no initialization, migration, capture, recovery or automatic
observer. Ordinary status/blame/stats and agent hooks retain their existing
behavior. Native attribution remains disabled. Permanent shared Git/jj command
semantics are a later delivery phase.

`status` checks current collection opt-in before workspace discovery or journal
opening. It requires a jj workspace, then reuses the complete current-source
policy, retained capture and saved-state verification in
`read_registered_admission_state`. Its cursor and optional latest receipt describe
historical observations. The checkout relation remains an exact original-cutoff
anchor or outside that cutoff; this command cannot establish attribution readiness.

`receipt` takes two exact 64-character lowercase hexadecimal identities. It uses
`read_native_admission` without workspace discovery or a fresh source-policy
claim. It validates the complete saved registration, current packet and any
distinct requested packet before returning the requested historical receipt.
A missing admission returns JSON null only after those integrity checks succeed.
The receipt generation is its original generation, not the current admission cursor.

Valid native diagnostic requests support Linux/macOS. Other platforms reject
before configuration, workspace or journal access. Syntax and help handling are
platform-independent. Unknown, duplicate, missing or extra arguments reject;
`--json` is required and options may appear in any order after the action.

## Journal opening

`JjObservationJournal::open_read_only_at_path` opens an existing database with
SQLite READ_ONLY and the existing memory-limit helper. It applies connection-local
busy timeout, foreign-key and in-memory temporary-storage settings. A single
DEFERRED read transaction verifies exact current schema version 4 using the
existing opaque/native/registration/admission schema checks, then releases the
snapshot. It never invokes the initializer, creates parent directories, upgrades
old versions or executes the prepare-only foreign-key binding inserts.

Empty paths, the SQLite special name `:memory:` and a leading `file:` URI reject.
Bundled SQLite enables URI processing globally, so omitting the URI open flag
alone cannot prevent `immutable=1` or other URI parameters. An ordinary absolute
path or `./file:...` can address a literal filename containing a colon where the
platform supports that name.

Read-only means SQL data and schema preservation. SQLite may create or maintain
WAL/SHM sidecars to read a live database. The connection must observe committed
WAL entries and retain normal snapshot locking; it never marks a mutable journal
immutable. See SQLite's [WAL documentation](https://www.sqlite.org/wal.html) and
[connection flags](https://www.sqlite.org/c3ref/open.html).

The opener verifies the existing schema contract, not all stored records or
filesystem provenance. The selected readers retain their own bounded native and
checksum validation. A read-only handle cannot be used to commit journal writes.
Existing writable opening and migration behavior remain unchanged.

### Existing-only writable opening

`open_existing_at_path` is a separate writable prerequisite for a future daemon
consumer. It uses SQLite READ_WRITE without CREATE, rejects special filenames,
and verifies the resulting connection is actually writable: SQLite can otherwise
fall back to read-only access. It requires preexisting WAL mode and exact current
schema; it never creates directories/databases, migrates schemas or converts the
journal mode. Connection settings retain FULL synchronous durability, the existing
memory limit, foreign keys, 250-ms busy timeout and temporary storage in memory.

Opening performs no application SQL/schema mutation. Normal SQLite recovery or
checkpointing may change physical database/sidecar bytes without changing logical
contents. Source authorization, identity and native evidence verification remain
with the caller. Existing read-only and explicit creating/migrating APIs retain
their behavior; no CLI or background worker is switched to this opener yet.

Tests preceded implementation: missing-API failures, followed by six failures
against a temporary control using the creating opener. That control passed two
other groups; grouped failures stop at their first assertion. A separate Unix
permission regression then reproduced SQLite's actual read-only fallback against
the first candidate before the explicit guard was added. The guarded candidate
passed all nine public groups and the private connection-settings case on macOS.
The fallback fixture reports unavailable only when effective uid zero actually
bypasses file permissions. Logical SQL, live committed WAL, no creation/migration,
mode preservation and actual existing capture writes are covered separately.

Final local qualification passed a fresh build, 203 jj unit tests, 459 jj
integration cases (28 opt-in cases excluded), 24 integration policy cases,
formatting and Rust 1.93 all-target Clippy. Other-platform qualification is
tracked separately in CI.

## Output and budgets

JSON identifies schema version 1, backend `jj`, action, evidence scope and
`attribution_enabled: false`. Status returns the current cursor, optional latest
receipt and workspace name/relation. Historical inspection returns receipt
metadata, operation count and original-cutoff/root closure. Neither emits raw
operation/view bytes, operation descriptions, host/user metadata or saved paths.
Errors have a stable code and a message bounded to 1,024 Unicode characters, with
nonzero exit status. SQLite opening failures project the static operation and typed
error code, excluding SQLite's filename-bearing message. This is an opening-error
boundary, not a blanket redaction guarantee for every possible source error.

Each command has one cooperative five-second deadline and one cumulative 32-MiB
SQL read budget. Existing maximum selected-byte costs are 16.625 MiB for a
current-source status and 24.5 MiB for a distinct historical receipt. These bounds
exclude SQLite pages, peak heap and separate policy/discovery I/O. Busy timeouts
apply per SQLite lock wait; neither they nor cooperative deadlines can interrupt
a blocking system call. No Git Trace2 ingestion work is added.

## Qualification

The opener tests preceded production: missing-API RED was followed by a temporary
writable-constructor control that failed one private and seven public cases while
four controls passed. The read-only implementation passed all 12 cases, covering
exact schema, old/corrupt refusal, SQL write refusal and committed live-WAL reads.

After a test-import correction, all 11 ordinary CLI tests failed against the
missing dispatch. The first implementation required a helper-visibility compile
correction, then passed ten cases and exposed a filename-bearing SQLite opening
error in the eleventh. The shared opening-error projection fixed that failure;
all 11 passed without assertion changes. Compile/scaffolding corrections are
recorded separately from behavioral RED.

Final local macOS qualification passed 188 jj unit tests and 409 default jj
integration tests, with 23 explicit/child lanes ignored. The new pinned real-jj
CLI case was invoked separately and passed current status and historical baseline
receipt checks while preserving dirty bytes. The existing debug-context filter
passed 12 cases with three ignored. The policy filter passed 21 cases (including
two new CLI policy checks), six internal-spawn checks and 33 fork workflow policies
passed, and a fresh build, format check and Rust 1.93 all-target Clippy passed.
Independent reviews covered source boundaries, fixture isolation and output.

Earlier native admission/history/registration real-jj qualification remains
recorded in its decisions; those six lanes were not rerun for this increment.
These macOS results do not establish Linux or Windows runtime qualification.
The later [explicit capture increment](2026-09-08-jj-explicit-capture.md) implements
initialization/capture commands. [Finite observation](2026-09-08-jj-finite-observation.md)
and the [daemon observer](2026-09-08-jj-daemon-observation.md) are now implemented.
Native attribution remains a separate follow-up.

## Per-head admission metadata

Historical receipt, explicit capture and changed finite-observation results now
include `admission.head_closures`: sorted entries with `head_id`,
`reached_baseline_ids` and `reaches_root`. These are recomputed from verified native
evidence; existing aggregate fields, schema version 1, packet bytes and receipt
identities retain their meanings. Status, initialization, cursor and receipt
identity objects, unchanged observations and daemon control replies are unchanged.

The maximum 32-head by 32-anchor summary can exceed 128 KiB. Successful debug-output
test helpers allow 256 KiB per value; the finite-stream helper allows 8 MiB across
32 attempts. A native fixture covers every head reaching all 32 anchors together
with a 16,000-byte workspace requiring JSON escaping. These are bounded test-reader
allowances, not a new production output-size enforcement. Error and daemon-control
reply test caps remain 32 KiB.
