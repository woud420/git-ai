# Reconcile native jj observations without repeated-head writes

Status: accepted — implemented and locally qualified on macOS for the pinned jj profile.

Refs: [ENG-415](https://linear.app/polarcoordinates/issue/ENG-415),
[durable admission](2026-09-08-jj-native-admission.md),
[explicit capture](2026-09-08-jj-explicit-capture.md),
[finite foreground observation](2026-09-08-jj-finite-observation.md).

## Decision

Add `reconcile_registered_history` beside the existing admission API. It takes
the same caller-owned journal, workspace context, Config, absolute deadline and
cumulative SQL `ReadBudget`. Its `NativeReconciliationExpectation` combines the
existing exact admission expectation with the expected workspace name and
attachment ID. The existing explicit admission API remains unchanged.
Its result is `NativeReconciliationOutcome::Unchanged` containing verified
registered state, or `Admission` containing the existing admission outcome.
Linux and macOS use the qualified native reader; other platforms refuse.

This is one bounded attempt for a future asynchronous observer. It adds no
scheduler, watcher, inventory, CLI action, implicit initialization or cursor
refresh. Native attribution remains disabled. Explicit `capture` still permits
deliberate repeated-head packets under a fresh expectation.

Flow: [rendered reconciliation](../architecture/diagrams/jj-native-reconciliation.svg)
([Mermaid source](../architecture/diagrams/jj-native-reconciliation.mmd)).

## One retained attempt

Validate the bounded expectation and collection opt-in, open one retained history
session, apply full canonical repository policy and verify saved registration.
Require the selected workspace name and attachment to match the supplied target
before choosing a branch or collecting history.
Compare the fresh sampled head set with the canonical expected head set. The
saved initialization cutoff is historical and cannot supply the fresh sample.

For different heads, use the existing collection and admission path in that same
session. Keep complete closure to the original cutoff/root, borrowed preparation,
prior native verification, exact receipt before CAS, checked staged readback,
final retained checks and the sole commit. Recheck the supplied workspace target
in the complete IMMEDIATE snapshot before retry or staging and in the checked
owned result before commit. Do not reset budgets or retry a conflict.

For equal heads, acquire the existing IMMEDIATE admission transaction without a
requested admission ID. Verify complete registration, selected workspace,
baseline, latest packet and cursor, including gaps, selected-generation ambiguity
and the indexed higher-generation check. Pin transactional source/receipt,
baseline identity/profile/generation/heads and selected workspace name/attachment
to the initially owned registration and supplied target. Require the exact
expected generation and canonical heads. Equality of sampled heads alone cannot
authorize `Unchanged`.

Construct the result from the owned initial registration and bounded cursor/latest
receipt metadata, without cloning raw baseline or packet bytes. Hold the write
reservation through final retained source and deadline checks, then drop the
unstaged transaction. This path neither stages nor commits admission rows and
does not walk native parents. Errors return no result and release the reservation.

Both paths retain historical sampling semantics. Raw heads or checkout can move
after their successful sample while retained metadata checks still succeed.
There is no third head scan, atomic filesystem/SQL snapshot or ABA guarantee.

## Retry and race behavior

| Situation | Result |
| --- | --- |
| Generation zero and unchanged baseline heads | Unchanged; both admission row families stay empty |
| Current positive generation and unchanged heads | Unchanged; preserve latest receipt and all rows |
| Same heads supplied in another order | Compare as the same bounded canonical set |
| Manual repeated-head capture advanced the cursor | Stale reconciliation refuses even if sampled heads still match |
| Lost reply from a changed-head attempt, identical request recurs | Existing exact receipt retry; historical receipt and current cursor remain separate |
| Heads changed after a lost reply | Stale new request refuses; do not infer the old receipt |
| Unchanged candidate: valid registration or selected attachment replaced before the transaction | Refuse the old registration identity |
| Remembered workspace name or attachment differs on either branch | Refuse before returning a retry or writing progress |
| A returns after A→B with current expectations | A new observation; historical head membership is not a deduplication rule |

A later worker must pin immutable source/receipt/baseline/attachment across
restart and refresh. It must reject remembered generation rollback or changed
heads at an equal generation. Without remembered progress, coherent erasure or
rollback remains indistinguishable from valid earlier state. This operation
introduces no activation witness, automatic adoption or repair.

Workspace scope constrains the current attempt, not historical packet ownership.
Canonical packet identities remain source-wide and unchanged. An identical
changed request previously written by explicit capture can still return its
original receipt once current policy and the expected workspace are verified.
Checking only a returned attachment would be too late to prevent a write; the
target checks therefore run before commit inside the retained attempt.

## Bounds and observer limits

Unchanged still verifies current head/checkout evidence and the complete saved
latest packet. Missing unneeded raw ancestors may be tolerated; missing current
head or required checkout evidence refuses. Changed heads must close fully to
the original cutoff/root with the existing 256-pair/8-MiB raw and separate 8-MiB
encoded limits. Earlier admissions never become traversal terminals.

The source can remain unavailable once that original-cutoff distance exceeds
the caps. Neither faster polling nor retaining the cursor enables partial resume.
Chunked closure or certified terminals require a separately qualified design;
this increment does not complete catch-up or P2 observation.

Conservative selected-SQL-BLOB ceilings are 25.125 MiB for unchanged with latest,
41.75 MiB for changed fresh admission and 33.125 MiB for a distinct exact retry.
One future caller can reuse the writer's 48-MiB budget and cooperative five-second
deadline. These are not total I/O, peak heap or hard wall-clock bounds. Existing
native limits and policy lookup behavior remain unchanged; no Git/jj process
spawns or Trace2 ingestion work are added.

## Qualification

Tests preceded production. The initial core first failed on the missing API,
then passed 11 public cases, 11 private transaction cases and two pinned real-jj
layouts. The workspace-target refinement first failed on its missing API; an
explicitly target-ignoring control then produced two private and three public
behavioral failures. Grouped cases stopped at their first failed assertion, so
that control does not independently qualify later iterations.

The enforced implementation passed all 14 private and 14 public reconciliation
cases, plus both real jj 0.45.1 layouts. Cases cover no-op generation zero and
positive progress, changed admission and exact retry, replaced selected targets,
invalid and bounded target names, deadline/lock/source races, corrupt saved
evidence and original-cutoff traversal. Readback replacement is a joint guard
regression; independent source review also verified the final target comparison.

Final local qualification passed 202 jj unit tests and 434 jj integration cases
(26 opt-in cases excluded from that default run). Six opt-in real-jj cases were
then invoked explicitly across reconciliation, existing admission and debug
commands. The fresh build, 23 integration policy cases, six internal-spawn cases,
33 fork workflow cases, format check and Rust 1.93 all-target Clippy passed.
These are local macOS results; CI qualifies other platforms separately.
