# Reject Lite Mode

Status: proposed; accepting this change records the decision

Tracking issue: ENG-301

## Context

Upstream's opt-in `GIT_AI_LITE_MODE` skips authorship-note migration for
replacement commits produced by ordinary history rewrites. It keeps enough
working-log movement for later edits, and explicit CI attribution remains
available, but rebases, amends, cherry-picks, reverts, divergent resets, and
`update-ref` restacks can leave rewritten commits without the attribution that
was attached to their sources.

That tradeoff conflicts with this fork's rewrite contract. The contract treats
attribution conservation across Git-proven surviving content as a core
invariant, and it already requires bounded, batched Git work for every rewrite.
The fork has also invested in targeted rewrite memory and latency improvements,
so a broad fidelity-off switch is not the first response to a performance
problem.

## Options considered

### Adopt the upstream mode

This provides an emergency performance escape hatch, but only after porting and
maintaining its complete rewrite matrix and working-log safeguards. Enabling it
would knowingly remove attribution from supported, everyday Git operations.

### Add a partial or hybrid mode

A partial mode retains most of the implementation and test burden while making
the user-visible contract depend on which rewrite path ran. It is harder to
explain and easier to misconfigure without preserving the upstream mode's
simple performance tradeoff.

### Reject the mode

Keep rewrite attribution unconditional and address measured bottlenecks with
bounded, targeted improvements that do not change attribution semantics.

## Decision

Do not add or advertise lite mode. `GIT_AI_LITE_MODE` is not a supported
configuration surface in this fork, and daemon-handled rewrites continue to
migrate authorship notes whenever the immutable evidence required by the
rewrite specification is available.

Classify upstream lite-mode commits and their follow-up fixes as intentionally
not applicable in future reconciliation audits.

## Consequences

- Users retain one rewrite-attribution contract across supported Git
  operations; there is no opt-in path that silently weakens it.
- Rewrite performance and memory regressions must be measured and fixed at the
  bounded batch, scheduler, or persistence layer instead of bypassing note
  migration.
- The fork does not inherit the configuration, integration matrix, or
  working-log compatibility burden of a mode it does not intend to expose.
- A repository with intolerable rewrite cost has no fidelity-off escape hatch.

Reconsider this decision only after a reproducible supported-repository
workload exceeds a stated latency or resource budget after targeted
optimizations. Any replacement proposal must quantify the gain, name the exact
attribution loss, remain opt-in, and cover every affected rewrite and
working-log path before it can ship.
