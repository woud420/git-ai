# Architecture

Structure and boundary notes for git-ai. The authoritative developer map
(non-negotiable rules, module overview, data flow) lives in the repo-root
`AGENTS.md`; this directory holds the deep-dive specs.

## Core model

A single binary dispatches on `argv[0]`: invoked as `git` it is a transparent
proxy to the real git binary; invoked as `git-ai` it serves direct subcommands
(`src/main.rs`). All git integration is **trace2-driven and asynchronous** —
the shared daemon ingests trace2 event streams, orders them, and performs all
attribution side effects off the critical path.

Checkpoint → working log → authorship note:

1. Agent presets call `git-ai checkpoint` before/after edits; character-level
   attributions are computed against HEAD or the last checkpoint.
2. Checkpoints land in `.git/ai/working_logs/<base_commit>/`.
3. After `git commit`, the daemon generates an `AuthorshipLog`
   (schema `authorship/3.0.0`) and stores it via the configured notes backend.
4. Rewrite operations (rebase, cherry-pick, reset, stash, …) migrate notes and
   working logs based on exact ref transitions from the reflog cursor model.

## Specs in this directory

- [daemon-trace2-ingestion-spec.md](daemon-trace2-ingestion-spec.md) — how the
  daemon learns which git commands ran and establishes exact ref transitions.
- [rewrite-ops-spec.md](rewrite-ops-spec.md) — how authorship survives history
  rewrites once transitions are known.

Related specs still at `docs/` root: `attribution-fuzzer-spec.md`,
`rewrite-simplification-spec.md`.
