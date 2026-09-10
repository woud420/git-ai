# One-target daemon observation of native jj history

Status: accepted — implemented and locally qualified on macOS for the pinned jj profile.

Refs: [ENG-415](https://linear.app/polarcoordinates/issue/ENG-415),
[finite foreground observation](2026-09-08-jj-finite-observation.md),
[reconciliation](2026-09-08-jj-native-reconciliation.md),
[explicit initialization and capture](2026-09-08-jj-explicit-capture.md).

## Decision

Add an experimental, explicitly enabled native observer to the existing daemon.
It can continue after its client exits and restart from verified journal progress.
It serves one saved target per daemon home, including a home selected through
`GIT_AI_DAEMON_HOME`. It does not enable native attribution.

```text
git-ai debug jj observer enable --journal PATH --json
git-ai debug jj observer status --json
git-ai debug jj observer disable --json
git-ai debug jj observer resume --json
```

Before enable, the repository must be allowed by the existing collection policy
and explicitly initialized through `git-ai debug jj initialize --journal PATH --json`.
Enable takes an existing current-schema WAL journal and the caller's workspace
location. It does not initialize a source, create or migrate that journal, change
an allowlist, or scan for other targets. Existing SQLite WAL bookkeeping may still
change sidecars. Ordinary journal spelling rejects empty paths, `:memory:` and
leading `file:` URIs.

Enable and resume may use the existing managed daemon startup policy. Status and
disable address a running daemon and return `daemon_unavailable` if it is stopped;
there is no offline intent editor. Status, disable and resume use the saved slot,
not the caller's current directory. All four commands require `--json` and reject
unsupported platforms before config, path or daemon access. The implementation scope is
Linux/macOS and the pinned jj reader profile.

Flow: [rendered overview](../architecture/diagrams/jj-daemon-observation.svg) and [Mermaid source](../architecture/diagrams/jj-daemon-observation.mmd).

## Durable intent, existing admission progress

Keep one bounded intent record in `jj-observer-intent.sqlite` under the daemon's
internal directory. It stores a format version, control revision, journal/workspace
locators, source and initialization receipt IDs, reader profile, original baseline
ID/generation, workspace name/attachment, enabled/disabled intent and an optional
blocking diagnostic. It adds no source-journal table and no second admission cursor.
The record is limited to 512 KiB; each lossless Unix locator is limited to 64 KiB
before hex encoding. These bounds do not replace the shared control-frame limit.

The native admission state and verified latest packet already hold durable source
progress. On daemon startup, load intent once. Missing intent means disabled;
unreadable or invalid intent means unavailable. An enabled, unblocked record
permits fresh policy, source, registration and saved-target validation. Successful
bootstrap establishes the verified current cursor in memory without rewriting
intent or incrementing its revision. Disabled and durably blocked records do not
start a native session.

Startup and explicit resume intentionally include valid manual admissions made
while the worker was stopped. This is source-wide progress, not recovery of the
worker's last request or proof that it was the only writer. A running session
instead retains its expected cursor and pinned target. A cursor conflict stops
that session; it does not refresh status and silently adopt another writer's work.

## Controls and runtime status

Initial enable derives identity from fully validated native state, then publishes
the enabled intent before permitting a tick. Explicit resume validates saved
locators and identity, clears the block, and increments the control revision before
starting from the newly verified cursor. A failed validation retains the known
prior intent and does not clear a terminal local stop.

Duplicate enable of the same enabled, unblocked target validates and compares the
candidate, then returns `already_enabled` without resetting the session cursor or
rewriting intent. A blocked target requires resume. Resume of an already running
session returns `already_active` without refreshing it. A different target may
replace the slot only after explicit durable disable and drain.

Disable prevents new scheduling immediately and invalidates delayed completions.
It publishes disabled intent and clears a stored block. If native work remains,
the reply reports `stopping`; `disabled` requires that work to drain. Repeated
disable is idempotent. An already-committing native packet may finish after disable;
the worker discards its cancelled runtime update and never compensates the packet.
Global daemon shutdown retains its existing bounded exit deadline. A timeout is
logged and is not an acknowledgement that a blocking native job was cancelled.
`bg await` retains its existing Git/daemon completion contract.

Every reply includes version/backend metadata and `attribution_enabled: false`.
Controller status separates desired intent from runtime:

| Field | Meaning |
| --- | --- |
| `revision` | Committed control revision; zero for a known empty slot, null if unknown. |
| `desired_intent` | Enabled or disabled; null when intent is unavailable. |
| `target` | Saved source/receipt/profile/baseline/workspace/attachment metadata, without locators. |
| `runtime` | Pending, active, stopping, blocked or disabled. |
| `in_flight` | A native job still occupies the one reserved slot. |
| `session_cursor` | Last verified in-memory generation and admitted heads, or null. |
| `last_error` | Bounded diagnostic and whether its block was persisted. |

Status reads controller metadata only. Active is a verified session state, not a
fresh claim about the filesystem or SQL at the time of the status request. A
session cursor can remain visible while stopping or blocked; it is cleared after
disable or when publication uncertainty invalidates cached authority. Command
errors use the same status data plus an `error` and exit nonzero. Transport failure
or timeout may lose a successful reply; inspect status instead of assuming rollback.

## Serial work and publication uncertainty

One job slot covers startup, enable/resume validation and reconciliation. Native
work runs off Tokio and Trace2, outside the short state lock. A separate mutation
gate spans the final reservation/revision check through joined intent publication.
Delayed success and error results cannot overwrite a later disable or replacement.
Control revisions are separate from admission generations; revision or reservation
exhaustion refuses further work rather than wrapping.

Validation uses a fresh five-second cooperative deadline and a 32-MiB selected-record
budget. Each tick uses fresh configuration, its own five-second deadline and one
48-MiB budget. It invokes the existing reconciliation operation with the saved
workspace target and session cursor. A successful result carries only the verified
current cursor forward, including when the returned receipt is a historical retry.
Drop raw evidence and transaction handles before waiting. The first tick runs
promptly; later ticks wait five seconds after completion, with no overlap or catch-up
burst. Unchanged ticks append no native packet or durable scheduling progress.

Any reconciliation failure stops the session and attempts a revision-guarded durable
block. A persisted block survives restart until explicit resume. A crash before that
best-effort block is committed may permit a new startup validation. Terminal blocking
and native admission are not a shared atomic transaction.

An error from attempted intent publication does not prove rollback: SQL commit can
precede a failing parent-directory sync. Stop locally, discard cached intent and
cursor authority, and expose null revision, desired intent, target and session cursor
with a `persisted: false` diagnostic. Retain an occupied job until it drains, reporting
stopping and then blocked. Status remains available; enable/resume refuse, and disable
can reaffirm local cancellation but cannot claim durable disabled from unknown state.
Only restart reloads the actual store. If it contains enabled, unblocked intent,
normal startup validation may resume current source progress. There is no in-process
publication retry, repair or adoption from the discarded cache.

## Limits and follow-up

Every changed reconciliation still proves complete ancestry to the original cutoff
or root. The 256-pair and existing raw, semantic and encoded limits can permanently
prevent further admission. More frequent polling, daemon restart and explicit resume
do not shorten that history. There is no partial frontier, automatic rebaseline or
scalable catch-up in this slice.

The budgets count selected payload charges, not combined RSS, all SQLite work or
policy/config include reads. Deadlines are cooperative and cannot interrupt blocking
filesystem or policy work. Coherent rollback/erasure of local durable state remains
outside independently witnessed protection. Default metadata omits raw records and
locators; existing bounded native errors do not establish blanket redaction.

This connects bounded observation to the shared executable and actual daemon. Native
ordering for attribution, shared checkpoint/blame/stats behavior and scalable history
remain separate work. Native attribution is disabled throughout.

## Qualification

Local macOS qualification with jj 0.45.1 passed:

- 38 focused unit cases covering paths, intent persistence, control replies, job ownership and recovery guards.
- The complete Rust unit suite: 2,918 passed, three ignored.
- All jj integration regressions: 473 passed, 30 ignored, including six portable CLI cases and eight dedicated TestRepo daemon cases.
- Both explicitly invoked real-jj observer layouts, colocated and non-colocated, passed admission, client-exit, restart and original-cutoff checks.

Tests preceded the new store/controller/CLI implementation. Additional regressions reproduced stopped-completion acceptance and volatile resume errors before their fixes. The publication-uncertainty case forces a valid external intent change and checks a real failed compare-and-swap; it does not inject a post-commit filesystem-sync failure.

The preservation tests drain the existing traced Git readiness probe before taking filesystem snapshots. Their original assertions remain intact; observer production code was unchanged when correcting that fixture race.

Rust 1.93 all-target lint and Windows GNU cross-compilation with the installed Rust 1.97.1 toolchain passed. Native runtime qualification is on macOS; the Windows check covers portable CLI/control compilation. Formatting, 25 repository-policy cases and all 33 fork/documentation-policy cases also passed.
