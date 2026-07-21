# Persistence Model

One documented source of truth per persisted fact. There must never be two
undocumented authorities for the same fact; if a change introduces a second
writer or store for anything below, update this table in the same PR.

| Fact | Authoritative store | Caches / derived copies | Retention |
|---|---|---|---|
| Authorship notes (sqlite backend, default) | `~/.git-ai/internal/notes-db`, rows `origin='local'` | `refs/notes/ai` (only if exported/migrated); read-fallback backfills as `origin='cache'` | local rows never evicted |
| Authorship notes (git_notes backend, opt-in) | `refs/notes/ai` in the repo | notes-db `origin='cache'` rows | git-owned |
| Authorship notes (http backend, opt-in) | remote server (see `notes-backend-spec.md`) | notes-db `origin='queue'` (pending→uploaded cache) | uploaded rows kept as cache; cache evicted ≥90d over 10k rows |
| Working checkpoints | `.git/ai/working_logs/<base_commit>/` JSON | none (daemon FamilyState watermarks reference them) | migrated/renamed on HEAD moves; consumed by post-commit |
| Ref/worktree state | git itself | daemon `FamilyState` (in-memory, re-derivable), reflog cursor offsets | reconstructable |
| Transcript positions | streams DB (v4): per-session watermarks | — | advanced monotonically; sessions re-scanned from watermark |
| Transcript content | the agents' own transcript files (never copied) | redacted events → metrics DB (allowed repos only) | agent-owned |
| Metrics events | metrics DB (v5) | — | pruned by age; upload queue retries ≤6 then parks |
| Bash tool-use provenance | bash-history DB (v2) | — | pruned |
| Prompts / CAS queue | internal DB (v3, legacy — queue dormant) | — | candidate for removal in P9.5 |
| User config | `~/.git-ai/config.json` | `CONFIG` snapshot (per-process); daemon uses `Config::fresh()` | user-owned; `git-ai uninstall --purge` deletes |
| Credentials | OS keyring (flagged) or config `api_key` / `GIT_AI_API_KEY` | in-memory token cache | masked in all serialization |

Known multi-source tension (accepted + mitigated): under the sqlite-default
backend, a teammate's pushed `refs/notes/ai` update is only observed via the
read-fallback (which backfills `origin='cache'`) or `git-ai fetch-notes`;
`origin='local'` rows always win upserts. Backend switches go through
`git-ai notes migrate --to <backend>` — never by hand-editing stores.
