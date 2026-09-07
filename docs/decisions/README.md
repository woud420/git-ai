# Decisions

Decision records, design snapshots, implementation plans, and supporting
research, one dated file per topic (`YYYY-MM-DD-<topic>.md`, with design docs
usually suffixed `-design.md`). Each record has exactly one `Status:` line in
its header.

## Lifecycle

- `accepted`: an adopted decision or actively maintained plan. Its rationale is
  current, but maintained contracts, source, tests, and contributor docs take
  precedence for commands and implementation details.
- `proposed`: a future design that has not been accepted as current behavior.
- `historical`: implementation evidence, incident analysis, or a completed or
  abandoned plan. It is retained for provenance, not current instructions.
- `superseded`: a design replaced by a newer contract, implementation, or
  decision named in its status line.

Implementation plans and design specs formerly under `docs/superpowers/`
now live here. Root-level `docs/*-plan.md`, `*-analysis-*.md`, and
`*-worklog-*.md` files predate this directory and remain in place.

Commands and unchecked checklists inside `historical` records are preserved
evidence, not current instructions. In particular, old `task ...` commands are non-operative.
Use the repository [Makefile](../../Makefile) and
[contributor guide](../../CONTRIBUTING.md) for the maintained GNU Make command
surface.

The current operational and architectural authorities are the root `AGENTS.md`
and `README.md`, the indexes under `docs/architecture/` and `docs/contracts/`,
and the source and tests they cite. Decision records explain why those surfaces
have their present shape; they do not override them when details drift.

When making a non-obvious architectural choice, record it here: context,
options considered, decision, consequences, and an initial lifecycle status.
