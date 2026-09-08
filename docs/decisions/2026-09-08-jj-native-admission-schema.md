# Additive storage schema for jj native admission (ENG-415)

Status: accepted — additive migration implemented and locally qualified on macOS.

## Boundary

Journal schema 4 reserves two empty tables for durable, source-scoped native
history admission. The migration preserves the bytes and identities of existing
opaque observations, native baselines, source registrations and workspace
records. It creates no admission receipt, generation-zero row or observer.
Native attribution remains disabled.

The [registered history collector](2026-09-08-jj-native-history-collection.md)
already returns complete historical evidence against the original baseline or
virtual root. Its returned value has no live source lease or write authority.
Durable admission will require a separate retained collection, checked SQL
transaction and final source check; those operations are outside this migration.

## Table contract

`jj_native_admissions` stores `source_id TEXT`, `admission_id TEXT`,
`generation INTEGER`, `record BLOB` and `checksum TEXT`, all nonnullable.
Its primary key is `(source_id, admission_id)`, and `(source_id, generation)`
is unique. Its immediate `NO ACTION` foreign key refers to
`jj_native_registrations(source_id)`.

`jj_native_admission_states` stores `source_id TEXT`, `admission_id TEXT`,
`state BLOB` and `checksum TEXT`, all nonnullable. The source is its primary key.
Its immediate `NO ACTION` composite foreign key refers to
`jj_native_admissions(source_id, admission_id)`.

Opening verifies exact columns, primary-key positions, foreign-key targets,
column order and actions, and full ascending binary unique indexes. The existing
bounded metadata helpers also reject defaults, generated columns, extra columns,
partial or expression indexes, missing uniqueness and extra indexes. An
equivalent explicit binary unique index may implement the generation constraint.
Foreign-key inspection does not certify deferrability; creation uses the
immediate form. Preparing each table's fixed insert statement without executing
it also checks that SQLite can bind the parent key's declared collation.
No row or trigger program runs for these binding checks.

Schema validation certifies the storage shape, not record contents. Existing
version-4 rows are left untouched even if their contents are opaque to this
migration. Canonical codecs, checked generations and native evidence validation
belong to the subsequent admission reader and writer.

## Upgrade and rollback

The existing single IMMEDIATE initialization transaction performs every step.
A fresh journal creates version 1 and advances through versions 2, 3 and 4.
Existing versions 1–3 follow the remaining steps in the same transaction.
Version 3 creates both new tables unconditionally; even a correctly shaped
preexisting admission table is rejected. There is no adoption or data backfill.

Each version update must affect exactly one row and read back as the exact typed
version string. Any DDL, shape or version failure rolls back the entire upgrade,
including earlier steps from an older starting version. A version-4 journal
with an incomplete or malformed admission schema is refused without repair.
Earlier schema-3 builds refuse a schema-4 journal. This migration provides no
backward conversion; it does not change Git attribution storage.

Registration's existing absent-source check gains two fixed indexed source
presence probes, one per new table. An orphan admission or state row inserted
with foreign keys disabled must prevent that source ID being treated as unused.
Other sources remain independent. These negative checks create no registration
authority and do not inspect or certify admission payloads.

No generation-zero record is introduced. The later admission reader may derive
generation zero only from a complete valid registration and absence of both
admission row families for that source. Coherent erasure of both families remains
indistinguishable from first use to a caller without a retained nonzero cursor.
This schema does not provide rollback detection or automatic recovery.

## Verification

Against the unchanged schema-3 implementation, the 22 new integration cases
produced ten behavioral failures. The frozen-v3 native/opaque compatibility
case already passed. A separate private complete-registration snapshot control
also passed before migration. The new orphan regression failed because the
existing absent-source check accepted the orphan source. Only afterward were
the actual query-plan tests installed; they produced exactly the two expected
missing-constant compiler errors. All 22 new integration cases and four private registration checks passed the
first implementation run. The complete default jj suite then passed 367 integration
cases (20 explicit/child lanes ignored), including all 80 schema and individual
record regressions; 119 jj unit cases also passed. One older reopen test needed
its schema comparison updated: old payloads remain exact across migration, and
full schema snapshots remain exact across subsequent version-4 reopens.

Two explicit pinned-jj history cases and two registration cases passed across
both layouts, as did 18 policy-filter cases, six internal-spawn checks and 33 fork
workflow policies. Fresh build, formatting and Rust 1.93 all-target lint passed.
Lint identified one redundant borrow in the new fixture control; it was removed
and that control was rerun. Independent source review confirmed the three-file
production change and the actual indexed existence-query plans.

The independently generated
schema-3 fixture retains the complete committed version-2 SQL prefix, all six
existing rows and their DDL, and adds canonical original source/workspace records.
Independent calibration checked seven payload checksums and canonical round
trips, the complete joins and guard hashes, and two native operation/view pairs.
Its physical locators and identities are synthetic; it makes no current-source
claim.

The tests target logical row/schema preservation and transaction rollback.
They do not simulate power loss or require byte-identical SQLite WAL files.
