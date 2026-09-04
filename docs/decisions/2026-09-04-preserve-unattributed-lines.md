# Preserve Residual Unattributed Lines

Status: proposed; accepting this change records the decision

Tracking issue: ENG-292

## Context

An authorship note distinguishes three materially different states:

- AI attribution backed by checkpoint or recovery evidence;
- known-human attribution backed by an explicit editor checkpoint; and
- no attestation, reported as unknown or untracked.

Upstream's terminal recovery pass converts every line left without an
attestation after other recovery into known-human attribution. That removes
unknowns from output, but it changes `known_human` from an evidence claim into
a fallback assumption. In this fork, untracked additions are deliberately
counted as holes in attribution coverage, and the `authorship/3.0.0` note format
lets consumers distinguish an `h_` attestation from no attestation.

The existing recovery pipeline already has bounded solvers for evidence that
can identify an AI session. This decision concerns only the residual lines
left after those solvers have run.

## Options considered

### Convert every residual line to known human

This produces simpler human-versus-AI totals, but it asserts provenance that
was not observed. It also makes existing notes and consumers silently disagree
about what a known-human attestation means.

### Add an assumed-human attribution state

A distinct state would preserve the difference between observed and assumed
human work, but it requires an authorship schema and compatibility design. It
does not justify reusing `known_human` in the current schema.

### Infer human attribution only near known-human lines

Adjacency narrows the assumption but does not turn it into evidence. File and
commit boundaries can contain interleaved edits, so proximity alone is not a
safe provenance claim.

### Preserve residual lines as untracked

Keep the current evidence boundary: recovery may fill a line only when a
solver has qualifying evidence, and an uncovered residual stays unattested.

## Decision

Preserve residual unattributed lines as untracked. Do not add a terminal pass
that manufactures known-human attestations, and do not treat missing
attestations as evidence of human authorship.

`known_human` continues to mean that an editor integration explicitly observed
human work. Evidence-backed AI recovery may continue to reduce unknowns, but
the absence of qualifying evidence remains visible in notes, blame, stats, and
tests.

Classify upstream's blanket terminal known-human recovery as intentionally not
applicable in future reconciliation audits.

## Consequences

- Attribution coverage can remain below 100%, accurately signaling incomplete
  evidence instead of improving the number through an assumption.
- Existing `authorship/3.0.0` consumers retain the meaning of `h_` attestations
  without a schema or migration change.
- Commit, squash, and rebase flows keep the same line-level expectations:
  uncheckpointed new or rewritten content remains untracked.
- Presentation layers may explain untracked lines, but must not relabel them as
  known human in persisted data.

Reconsider a separate `assumed_human` state only through an explicit schema and
consumer-compatibility proposal. That work must preserve the proven-human and
unknown states, define migration and display behavior, and never reinterpret
existing notes.
