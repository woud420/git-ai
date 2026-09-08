# Finite foreground observation of native jj history

Status: accepted — implemented and locally qualified on macOS for the pinned jj profile.

Refs: [ENG-415](https://linear.app/polarcoordinates/issue/ENG-415),
[reconciliation](2026-09-08-jj-native-reconciliation.md),
[explicit initialization and capture](2026-09-08-jj-explicit-capture.md).

## Decision

Expose the existing reconciliation operation through an experimental, finite
foreground command. The process observes one explicitly initialized jj source;
it does not enable attribution or register a background worker.

```text
git-ai debug jj observe --journal PATH --json \
  --expect-source SOURCE_ID --expect-initialization-receipt RECEIPT_ID \
  --expect-baseline BASELINE_ID --expect-generation N \
  --expect-head OPERATION_ID [--expect-head OPERATION_ID ...] \
  --expect-workspace NAME --expect-attachment ATTACHMENT_ID \
  [--attempts N] [--interval-ms N]
```

The default is one attempt. Attempts are limited to 1–32; the interval defaults
to 1,000 ms and accepts 250–60,000 ms. Numbers use canonical decimal syntax.
These conservative experiment bounds are not production performance targets.
The first attempt runs immediately. Later attempts wait after the preceding
result; there is no trailing wait, overlapping work or catch-up burst.

The command retains the source, registration, baseline and workspace target
supplied by the caller. It advances only the generation and heads from each
successful verified current cursor. It never obtains a new target or cursor
from status after an error. Existing exact historical retries remain valid.

Flow: [rendered observer](../architecture/diagrams/jj-finite-observation.svg)
([Mermaid source](../architecture/diagrams/jj-finite-observation.mmd)).

## One process, serial attempts

Discover the initial workspace context once and keep one writable journal open.
Each attempt loads fresh configuration, checks collection permission and uses
one five-second cooperative deadline and one 48-MiB selected-payload budget.
Initial discovery and journal opening share the first attempt deadline. Fresh
configuration cannot change the frozen workspace context. An empty allowlist
returns `collection_disabled` even after earlier successful results.

Reconciliation preserves its original-cutoff ancestry, saved-evidence checks,
workspace target checks and transaction boundaries. Release the returned raw
evidence and native capture state before writing output or waiting. Keep the
frozen context, open idle journal and bounded cursor metadata between attempts.
The observer adds no Git/jj subprocess logic or Trace2 ingestion work;
reconciliation retains its existing cold repository-policy lookups.

Each successful attempt emits and flushes one JSON line, with a one-based
`attempt`, the verified cursor and selected workspace name/attachment. Outcomes
are `unchanged`, `admitted` or `already_admitted`. Unchanged results include the
latest receipt or null; changed results include the existing admission metadata. Every
record reports `attribution_enabled: false`. There is no final summary record.

A semantic error emits the existing bounded error envelope and stops with a
nonzero status. A failed output write or flush stops without another attempt or
a recovery response on the failed output stream. Earlier committed observations
remain durable. The caller may lose a committed reply; output and SQL are not
an atomic transaction. Terminating the foreground process leaves no worker.

## Limits and follow-up

The writer can create or migrate a journal before a later policy or registration
refusal. It never initializes a source implicitly. Configuration/include reads
and filesystem calls keep their existing limits. The deadline starts before
fresh configuration and is checked cooperatively; it cannot interrupt blocking
calls or bound stdout and waiting in wall-clock time.

Every changed attempt still needs complete ancestry to the original cutoff/root.
The 256-pair and raw/encoded byte limits can permanently prevent later capture.
Polling more often and restarting this command do not shorten that history.
Missing required ancestry, target changes and cursor conflicts stop the run;
there is no rebaseline, partial resume or automatic recovery.

A separately enabled [daemon observer](2026-09-08-jj-daemon-observation.md)
adds unattended scheduling and intentional restart from verified source progress.
It retains the same original-cutoff limitation; scalable closure remains pending.
Shared ordinary checkpoint, status, blame and stats behavior still depends on
the ordering and attribution phases. This finite command keeps its existing
foreground lifetime and caller-supplied expectation semantics.

## Qualification

Tests preceded production: the old command rejected all eight native observer
groups and three positive portable groups. After implementation, all 16 observer
groups passed. Negative parser groups also passed before implementation because
the old command rejected the unknown action; the passing final run exercises
their individual validation cases.

Local qualification passed a fresh build, 202 jj unit tests, 450 jj integration
cases (28 opt-in cases excluded), four explicitly invoked real-jj debug cases,
24 integration policy cases, six internal-spawn cases, 33 fork workflow cases,
formatting and Rust 1.93 all-target Clippy. The two new observer layouts, an existing colocated diagnostic case and
an existing colocated writer case used jj 0.45.1.

The new real-jj tests pause the observer after its first flushed result while
jj creates a successor, then resume it. They qualify a change between attempts,
not arbitrary mutation during active capture. The pause is submitted without a
separate stop acknowledgement, using a generous interval.

The closed-pipe case requires nonzero termination and permits at most one already
committed new packet. It cannot independently detect an extra read-only attempt;
source review separately verifies that output failure returns before another
attempt or sleep. Timing observations include deliberate waits and test work;
they are not production performance targets. Other cases cover current-cursor
carry on historical retry, fresh policy revocation, source/cursor conflicts,
missing parents, original-cutoff overflow, idle transaction release, dirty-file
preservation and strict portable parsing. Other-platform qualification remains with CI.
