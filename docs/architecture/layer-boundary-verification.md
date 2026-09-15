# Layer boundary verification

Scope: [audit and decisions](layer-boundary-audit.md), tracked by
[ENG-464](https://linear.app/polarcoordinates/issue/ENG-464/tighten-existing-layered-architecture-boundaries).
Baseline: `f138e678802b886603b5504fd789768fef506113`.

## Review sequence

| Commit | Verified change |
|---|---|
| `1d8b981a6` | Evidence and ordered corrections recorded before production edits. |
| `02d67f1a8` | Reducer deletion/raw-observation characterization and TestRepo ref deletion/recreation passed against the original helper. |
| `3cfab2669` | Original OID file moved byte-for-byte; old public path re-exported. Reducer/history/telemetry imports use the existing value owner. |
| `ab59f2069` | Existing import policy tightened; negative fixtures failed before enforcement and passed afterward. |
| `6826709b3` | Two coordinator method signatures/bodies moved byte-for-byte; callsites and lock lifetime unchanged. |
| `3fe3ad910` | Review round 1: comment-masked import regression demonstrated, then rejected by the bounded scanner. |
| `bdcf72d0f` | Current architecture and persistence claims reconciled with source. |
| `2cb938122` | Review round 2: restore backend/export facts and stable file/symbol references; all 33 existing documentation-policy tests pass. |

The OID module's before/after SHA-256 is
`9fac049903f1717f32442e73c18ec402ed382563c904c489ef3c6ea5a309802d`.
No production Git spawn, IO, new type or critical-ingress work was added.
Coordinator sizes changed from drain/ingest/seq 508/325/567 to 535/306/557 lines;
no file-size baseline changed. Public serialized fields retain their representation.

## Characterization and structural gates

All checks use repository GNU Make targets. Baseline reducer (10), OID unit
checks (68), analyzers (23), coordinator drain (11), branch lifecycle (1),
and same-family ordering (1) passed. After movement, the corresponding checks
passed again, as did the new TestRepo ref test and file-length policy.
The final layer-import-policy group has 11 passing tests, including its
whole-source check and negative syntax/effect fixtures.

The reducer matrix pins empty/whitespace values, 40/64 zero OIDs, malformed
lengths, padded zeros and nonzero values. It asserts raw `RefChange` retention,
ref-map contents and sequencing. The TestRepo test asserts line attribution
after each commit, ref deletion, recreation and checkout.

## Final repository gates

Runtime checks use Rust 1.98.1. Lint/format/docs use installed Rust 1.93.0,
matching the CI workflow's MSRV selection; Windows compilation uses installed
Rust 1.97.1 with `x86_64-pc-windows-gnu`. Separate compiler output directories
prevent toolchain artifact mixing. Test subprocesses inherit the existing
Cargo cache and offline mode; TestRepo daemons need local socket permission.

| Gate | Result |
|---|---|
| `gmake build` | Passed |
| `gmake lint` (CI MSRV) | Passed |
| `gmake fmt`, `gmake format-check` | Passed |
| `gmake doc` (warnings denied) | Passed |
| `gmake check-windows` | Passed; compilation only |
| `RUST_LOG=info gmake test CARGO_TEST_ARGS=--no-fail-fast` | Passed: 7,283 tests; 0 failed; 119 ignored (including doctests) |

The first full run found four documentation-contract failures: backend labels
and exported Git Notes compatibility were lost, and stable path/symbol
references no longer matched the maintained contract. Tests were retained;
the documentation correction is verified by all 33 tests in the existing
`fork_workflow_policy` binary. The four failures corresponded to three
underlying documentation findings, all corrected. The separate round-1 source
review found and fixed one import-scanner regression; its documentation review
found and fixed one ambiguous path-reference issue.

Default host Clippy 1.98.1 rejects unchanged `chunks_exact(2)` sites in
`cli/hook_input.rs` and `model/jj_observer/paths.rs`. The same prescribed lint
target fails on the clean pinned base. No lint was suppressed; CI's pinned
MSRV lint passes. This is an inherited toolchain discrepancy.

### Repeat the gates

Use the Makefile in this checkout (there is no maintained Taskfile). Set
`CARGO_HOME` to an existing Cargo cache, `CARGO_NET_OFFLINE=true` and
`RUST_LOG=info`; allow
TestRepo local sockets. Select the toolchain bin directory first on `PATH`
and an independent `CARGO_TARGET_DIR` for each compiler. Run the table above
with Rust 1.93.0 for lint/format/docs, Rust 1.97.1 for Windows and Rust 1.98.1
for build/full tests. `gmake test TEST_FILTER=layer_import_policy TEST_THREADS=2`
repeats the dependency checks;
`gmake test CARGO_TEST_ARGS="--test fork_workflow_policy" TEST_THREADS=2`
repeats documentation contracts. Raw local evidence is retained under
`target/layer-audit-evidence/`; compiler outputs are untracked under ignored
`target/` directories.

The main integration suite passed 4,017 tests (111 ignored) with two test
threads. A later notes-sync binary failed two log assertions under inherited
`RUST_LOG=warn`; cache-content assertions passed. Both failures reproduce on
the clean pinned base, whose messages are emitted at `tracing::info!`. All 43
notes-sync tests pass with `RUST_LOG=info`, without source/assertion changes.
The final full run passed with that logging level and the Makefile default
of 12 threads. No tests are filtered out or assertions disabled.

## Limits and handoff

The import guard is lexical, not a complete Rust dependency resolver or a proof
of whole-model purity. Windows runtime and hosted CI were not exercised.
No production installation, remote push, pull request or deployment occurred.
Review the local commit sequence and audit before choosing a publication step.
