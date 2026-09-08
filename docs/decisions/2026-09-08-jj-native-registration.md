# Explicit native jj source registration (ENG-415)

Status: accepted — implemented and locally qualified on macOS; Linux/Windows CI is tracked separately.

## Scope

`register_current_state` joins a newly published source seal, a current-state
baseline and its first workspace in one explicit operation. A complete existing
registration returns `AlreadyRegistered` with its saved cutoff.
`reopen_registered_current_state` performs the corresponding read-only join.
Both APIs first require a nonempty collection allowlist, before opening C0
or accessing paths or policy configuration. They then require the existing
repository policy to allow the sampled repository. Neither changes the
allowlist nor enables attribution.

This increment supports the pinned `jj-simple-op-store/0.45.1` profile on Linux
and macOS. Other platforms reject before path or policy access. It introduces
no CLI dispatch, wrapper, Trace2 ingestion work, attachment mutation or recovery.
The registration APIs do not walk history. [Registered history collection](2026-09-08-jj-native-history-collection.md)
is implemented separately and locally qualified; it reuses the fresh
registration join and preserves the original saved cutoff. Standalone native
baselines remain namespace-scoped; individual registration-record readers retain
their orphan-inspection contract. Neither API substitutes those records for
complete source registration.

[Initialization flow](../architecture/diagrams/jj-native-registration-flow.svg)
and [saved-receipt flow](../architecture/diagrams/jj-native-registration-reopen.svg)
show the two paths. Their adjacent Mermaid sources are retained for editing;
both rendered diagrams were visually inspected.

## Initialization and commit boundary

After the platform dispatch, both Unix entry points check
`Config::has_allowed_repositories()` before creating a capture budget or opening
C0. This explicit opt-in gate also precedes the legacy debug self-check remote
exception. Unsupported platforms still reject at dispatch first.

C0 is a private retained capture session. The coordinator loads existing Git
configuration and remote policy, then matches canonical workspace/Git paths to
C0's retained directory identities.
This prevents a lexical path such as an allowed directory followed by `..`
from borrowing the wrong path authorization. Policy load errors deny the call.
Source-root and locator guards must be clear before publication; they are
negative conflict checks, not evidence of first use or lifetime identity.

C0 publishes through its retained shared-jj-repository descriptor. It releases
raw native evidence before mutation, creates `git-ai` once, writes the exclusive
`.registration.tmp`, synchronizes the file, performs a same-directory no-replace
rename to `registration`, and synchronizes the namespace and repository parent.
Exact readback and retained metadata/edge checks must succeed. Every C0 directory
closes before C1 starts; the created regular seal File remains open across this
transition so C1 can compare the actual created inode.

C1 freshly captures the same physical source/workspace and exact seal, then
rechecks policy. Its native heads and evidence become the explicit historical
cutoff. Parents beyond those cutoff anchors remain unverified; registration does
not silently certify ancestry. Heads or checkout movement after the sampled
boundary does not change the saved request.

Storage stages exactly four rows in one IMMEDIATE transaction: native baseline,
native source state, source registration and initial workspace. The installer
requires absence, checks every insert affected one row, rejects an extra initial
workspace, and reads back the complete bounded join. Encoded write buffers are
released before readback. Operations reverify the native bytes and saved scope,
then C1 rechecks retained edges, pointer/backend samples and the named seal.
Only then does the coordinator consume the staged handle's sole SQL commit.
Any earlier error drops the transaction and rolls back its rows.

The initialization receipt is SHA-256 of the canonical source registration
record. Source identity combines its random source ID and exact seal with the
checked saved source/workspace bindings. Path, device/inode, family keys and
negative guards alone are never promoted to source authority.

## Seal and incomplete states

The seal has a closed ASCII format with final LF:

```text
git-ai/jj/source-seal/v1
source_id=<64 lowercase hexadecimal characters>
reader_profile=jj-simple-op-store/0.45.1
```

The concrete seal is 141 bytes, with a 1 KiB read cap. The namespace must be an
observed mode-0700 directory owned by the effective user; the seal must be a
regular, single-linked mode-0600 file with the same owner. macOS extended ACLs
on these objects reject. This adds no owner policy for source ancestors.
Only the exclusively created leaf may be adjusted to mode 0600 by descriptor.

Every preexisting namespace needs a valid matching seal and complete saved SQL
registration. An empty directory, temporary-only state, malformed seal, orphan
seal or incomplete SQL is unavailable. A standalone baseline is not adopted.
Publication or later SQL failure leaves any created namespace, temporary or
final seal in place. There is no automatic deletion, adoption, repair or new
baseline. Filesystem publication and SQLite commit are separate durability
steps; a seal-only crash gap is intentional and requires future explicit recovery.

## Saved cutoff and workspace relation

Existing-source retry and reopen pass the same pre-capture opt-in gate and
retain the original receipt, attachment and baseline even when jj heads or
checkout advance. One read transaction joins the
source record, its original workspace, the requested workspace and native state.
When the requested workspace is original, it is decoded and charged only once.
Unselected workspace records are outside this bounded read's claim. Native
operation/view hashes are mandatory on reopen, even if local checksums match.
The saved binding, historical checkout and native baseline are verified before
the final retained metadata, directory-edge and seal checks. Only then is the
original receipt and fresh checkout context returned.

Fresh capture must match the saved physical/workspace bindings and prove that
the selected workspace belongs to its own native checkout view. Historical
checkout bytes are context, not permanent attachment identity. A fresh checkout
outside the saved cutoff returns `OutsideBaseline`; it supplies no ancestry or
attribution-readiness proof. Even `BaselineAnchor` records only exact membership
in the saved cutoff. The returned value is an owned sampled result, not a live
descriptor lease or a claim of currentness at return.

## Bounds and qualified limits

| Stage | Bound |
| --- | --- |
| Native evidence per capture | 32 heads, at most 32 anchors plus one outside-checkout pair; 1 MiB per envelope; 8 MiB retained anchor bytes |
| Capture and retained rechecks | At most two sessions; 10 MiB and 192 file-read attempts each, without resetting for extra rechecks |
| Directory work per session | 256 open attempts, components and retained edges; 72 raw head-directory calls; no namespace scan |
| Scoped native filesystem descriptors | 254 directories including scan streams, one retained seal File and one transient regular reader: at most 256 |
| Publication mutations | One mkdir, exclusive create, owned-leaf chmod and no-replace rename; 256 raw writes; eight aggregate raw sync attempts |
| Saved SQL records | 128 KiB per registration/workspace/state, 8 MiB encoded baseline; caller-owned aggregate `ReadBudget` |

C1 reads the seal twice, bracketing native capture and staging. Final rechecks
repeat retained metadata/edges and the seal, not a third heads/checkout sample.
SQL gates types and byte lengths before materializing payloads or scalar keys;
complete selected BLOBs remain charged after later validation failure, and
repeated reads charge again. These budgets do not measure SQLite pages or peak
allocator usage. Reopen uses one capture session and does no filesystem write.

The native capture, seal and storage paths spawn no Git/jj processes. Existing
policy loading is a separate explicit stage: its cold installation lookup may
perform a fixed-count Git subprocess, and configuration includes retain their
existing I/O semantics. Capture limits therefore do not bound policy bytes,
include fanout, elapsed time or process-wide descriptor usage. Deadline checks
are cooperative and cannot cancel blocked config reads or filesystem syncs.
There is no filesystem/SQL atomicity, ABA, coherent full-state rollback or
power-loss qualification claim.

## Verification

New APIs began with missing-symbol failures before implementation. Separate
behavioral regressions reproduced an unexpected initial-workspace insertion,
the empty-allowlist self-check exception and a successful rename followed by
deadline expiry before their fixes. Fixture setup and identifier corrections
are recorded separately from these behavioral regressions.

Local macOS qualification passed:

- 19 TestRepo registration cases and two explicitly invoked real-jj cases for
  colocated and separate stores, including initialization, reopen and retry
  after native heads/checkout advance.
- 59 focused storage/capture cases covering exact joins, native revalidation,
  rollback at each insert, bounded query plans, retained descriptors, ACLs,
  publication counters and fault injection.
- 323 default jj integration cases (18 explicit/child lanes ignored by that
  command) and 93 jj unit cases.
- 16 regular-file cases plus the invoked isolated child, 67 outbox cases,
  16 policy-filter cases and 33 fork workflow policies.
- Fresh build, Rust 1.93 all-target lint and format check.

The real-jj lane used 0.45.1 commit
`7c41cdeb16b6b321c64e789a966b6adf723816a5`. Its two parent tests explicitly
invoke their isolated helper; none was merely counted as an ignored success.
Linux/Windows runtime qualification belongs to CI. These results do not qualify
power-loss recovery, attachment mutation, durable admission or native attribution.
