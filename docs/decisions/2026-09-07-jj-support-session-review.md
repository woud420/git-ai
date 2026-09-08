# Session review: native jj proposal and foundation

Date: 2026-09-07 (America/New_York).
Scope: research, proposal and two flowcharts, Linear decomposition, and the first
local implementation slice in the woud420/git-ai fork.

## Findings

Sections 1–4: zero outstanding findings. The Linear project document and all
12 issues were read back. Canonical body order, three label axes, assignee,
parent/project routing and blocking dependencies were checked. Mermaid sources
match the proposal; both SVGs rendered and were visually inspected.

Section 5: one note.

1. **note** — The branch is local and ENG-414 remains In Progress pending
   publication/human review. The original main checkout retains its pre-existing
   untracked `autoresearch/` directory. No push or PR was requested or performed.

Sections 6–8: zero outstanding findings. Existing Git author identity was
checked; the remote is the intended fork. Focused secret/conflict-marker scans
and review of outgoing documents found no secret values or unrelated personal
data. No credentials, global installation, licensing or repository settings
were changed.

Section 9: two resolved environment findings.

2. **note, resolved** — Nested TestRepo binary compilation stalled. The existing
   prebuilt-binary test override was used after fresh builds; actual behavioral
   RED and GREEN logs were preserved.
3. **note, resolved** — Sandbox socket binding prevented a Git checkpoint
   regression from starting; permitting its isolated sockets produced a pass.
   Host Clippy 1.98 also warned on unchanged base code; the repository's CI
   toolchain, Clippy 1.93, passed without unrelated edits.

Sections 10–12: one scope note, zero blockers.

4. **note** — Full native jj attribution is intentionally still phased work.
   ENG-413's reader proof is next. The full integration suite and Linux/Windows
   qualification were not run for the paths-only slice. Those gates remain
   explicit in ENG-423. No skill misfire requiring a skill-improve handoff
   was identified.

## Git-state table

| Repo/worktree | Branch | Committed? | Pushed? | Stack/PR |
| --- | --- | --- | --- | --- |
| `/Users/jm/workspace/projects/git-ai` | `main` | No user changes committed | No | Original checkout preserved |
| `/Users/jm/workspace/projects/git-ai-jj-support` | `codex/jj-support-foundation` | Design and validated P1 in separate local commits | No | No PR; ordinary fork branch |

The implementation worktree was moved from `/private/tmp/git-ai-jj-support`
to the persistent sibling path after test processes finished. The source and
tested binary were preserved. Final Git read-back confirms the committed tree
and original-checkout preservation.

## Knowledge capture

Written and verified destinations:

- Architecture, decisions, source links and dependency map →
  `/Users/jm/workspace/projects/git-ai-jj-support/docs/decisions/2026-09-07-jj-support-design.md`.
- Runtime provenance, command recipes, TDD and verification outcomes →
  `/Users/jm/workspace/projects/git-ai-jj-support/docs/decisions/2026-09-07-jj-support-evidence.md`.
- Reproducible isolated jj experiments →
  `/Users/jm/workspace/projects/git-ai-jj-support/scripts/benchmarks/jj/probe_runtime.py`.
- Maintainable and rendered diagrams →
  `/Users/jm/workspace/projects/git-ai-jj-support/docs/architecture/diagrams/jj-support-{context,flow}.{mmd,svg}`.
- This review →
  `/Users/jm/workspace/projects/git-ai-jj-support/docs/decisions/2026-09-07-jj-support-session-review.md`.

The [Linear proposal](https://linear.app/polarcoordinates/document/native-jj-support-proposal-architecture-and-delivery-plan-f4fd7edf1f13)
and [ENG-412](https://linear.app/polarcoordinates/issue/ENG-412) provide the
persistent tracker entry points. Memory files were not modified.

## Audit-round fix plan

No unresolved fix plan is needed for P1. Independent review found and fixed
Git-file backing targets, incorrect common-directory inference, and mixed
workspace/store identities at dangling nested boundaries. Regressions were
observed failing before the fixes, then passed. Next implementation work starts
with ENG-413, not automatic enablement of jj attribution.

## Skipped sections

No numbered section was wholly skipped; checks were limited to surfaces touched
by this session. Deployments, hosted apps, PR/remote CI monitoring, release
publication, skill installation, and secrets provisioning were not applicable.
Section 11 used focused cross-artifact and independent source review; it did
not run a repository-wide consistency audit or full-suite test campaign.
