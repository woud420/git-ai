# Contracts

Public and external surfaces of git-ai: wire formats, storage formats, and
payloads that other systems (servers, CI, teammates' clones) depend on.

- [notes-backend-spec.md](notes-backend-spec.md) — HTTP contract a remote
  notes backend server must implement (bulk read/write of authorship notes
  keyed by commit SHA).
- [telemetry-streams-summary.md](telemetry-streams-summary.md) — summary of
  the telemetry stream pipeline and its event payloads.
- [telemetry-examples.md](telemetry-examples.md) — example telemetry payloads.
- `../../specs/git_ai_standard_v3.0.0.md` — the Git AI note format standard
  (authorship note schema `authorship/3.0.0`).
- `../migrations/` — note/session format migrations.

The CLI surface itself is documented in the repo-root `README.md`; privacy
guarantees for collected data are documented in `data-privacy.md`.
