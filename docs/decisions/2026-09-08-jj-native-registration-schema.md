# Additive storage schema for jj registration (ENG-415)

Status: accepted — additive journal schema; source registration and onboarding remain separate.

## Boundary

Schema version 3 adds empty source-registration and workspace-attachment tables
to the existing jj observation journal. Existing opaque observations and native
baseline records retain their exact bytes, checksums and receipt identities.
No row is backfilled or adopted as a registered source. This migration alone
creates no source marker, enables no collection and makes no source-continuity
or workspace-readiness claim.

## Table contract

`jj_native_registrations` contains `source_id`, `baseline_id`, `source_root_key`,
`record` and `checksum`. The source is its primary key; `source_root_key` is unique.
The immediate `NO ACTION` composite foreign key refers to the existing
`jj_native_baselines(source_id, baseline_id)`.

`jj_native_workspaces` contains `source_id`, `workspace_name`, `locator_key`,
`workspace_root_key`, `record` and `checksum`. Its primary key is
`(source_id, workspace_name)`. `locator_key` is globally unique, and
`(source_id, workspace_root_key)` is unique. Its immediate `NO ACTION` source
foreign key refers to `jj_native_registrations(source_id)`.

All scalar key columns are nonnullable TEXT and record/checksum storage is
nonnullable BLOB/TEXT. New key indexes use binary collation, ascending key order,
full indexes and the declared uniqueness. Equivalent explicit unique indexes
are accepted for the guard and locator constraints; missing or extra indexes
reject. New-table column, primary-key, foreign-key and index validation uses
bounded metadata queries. Foreign-key inspection validates targets, column
order and actions; it does not certify deferrability. Creation uses the immediate
form shown above. Two fixed insert statements are prepared, never executed, so
SQLite also verifies that each child foreign key can bind its parent key. A
regression isolates a parent column whose default collation differs from its
otherwise valid binary primary-key index: its pragma metadata passes, but its
foreign key cannot bind. Opening rejects this layout without probe writes.
Existing native column and foreign-key validation semantics remain intact.
The index checks certify key collation, not column defaults; later exact-key
queries must explicitly use binary comparison. Metadata queries bound returned
rows and string sizes. They do not bound SQLite's internal schema parsing or
page I/O.

The keys permit bounded negative conflict checks before initialization or
attachment. They never authorize a source or workspace. The root guards
hash platform plus the sampled repository/workspace root device and inode under
separate fixed domains. They exclude other mutable directory identities. This
allows retained local records to reject a missing-marker reinitialization after
a move, and prevents moving plus renaming an attachment from bypassing the
immutable first-version policy. A hash collision is a conflict requiring recovery.
Locator identity, device/inode equality and absence of a matching row cannot prove
that a repository has never been registered.

[Bounded record inspection](2026-09-08-jj-native-registration-records.md) now
provides canonical codecs and individual reads. [Explicit source registration](2026-09-08-jj-native-registration.md)
now joins a fresh source capture and seal with complete checked records and native
baseline evidence, installing the four initial SQL rows in one transaction. The
migration itself does not introduce publicly constructible registration proofs
or a model-layer dependency on capture code.

## Upgrade and rollback

Use the existing single IMMEDIATE schema transaction. Fresh initialization starts
with v1, creates the existing v2 native tables, then creates the v3 registration
tables. A v1 or v2 upgrade follows the remaining steps in the same transaction.
Only exact typed version strings are accepted; each version update must affect
one row and read back as the requested value.

Create new tables unconditionally when upgrading v2. A preexisting partial or
complete v3 table set under a v2 version is rejected rather than adopted. Any
schema, version-update or validation failure rolls the whole migration back,
including v2 changes made earlier in a v1-to-v3 attempt. No `IF NOT EXISTS`,
upserts, compensating deletes or data backfill are used.

A valid namespace-only v2 baseline remains readable by the existing baseline
and pure ancestry APIs after upgrade. It remains unregistered. Explicit registration
cannot infer physical identity from that baseline or silently adopt it. A source
marker with missing local registration/native state is unavailable, including
crash-before-SQL and erased-journal cases; migration never repairs that ambiguity.

## Verification

The initial 24 new schema cases first produced 12 behavioral failures against
schema v2; the frozen-v2 native/opaque receipt compatibility case already passed.
After implementation all 24 passed. A separate foreign-key binding case then
failed against that candidate before the prepare-only correction.

The corrected migration passed all 39 schema cases (14 existing and 25 new),
including whole-chain rollback, typed version/cardinality faults, partial-table
refusal, concurrent openers, required index shapes, and exact old receipt retry.
The frozen v2 SQL fixture was regenerated byte-for-byte using an independent
CBOR encoder and the existing native operation/view oracle; frozen v1 remains
verbatim. The broader default jj integration suite passed 284 cases, with 14
explicit/child lanes ignored by that command. Four journal unit cases and twelve source/storage policy cases also passed;
the policy filter additionally reran one ancestry case. Fresh build, Rust 1.93
all-target lint and formatting passed. Independent compatibility and resource reviews cleared the final
five production files; no new Git/jj calls or Trace2 work were introduced.

Logical schema/row preservation and rollback are covered. These tests do not
simulate power loss or require byte-identical WAL files. Opening validates the
schema, not registration record contents or source continuity.

The broader design also preserves saved initialization receipts when heads advance,
keeps historical checkout IDs separate from fresh readiness evidence, and counts
publisher plus capture descriptors under one coordinator bound. Record codecs,
seal publication and atomic SQL installation are implemented in their linked
contracts. Attachment mutation, explicit recovery and CLI integration remain
separate tests-first boundaries; filesystem publication and SQL commit are not atomic together.
