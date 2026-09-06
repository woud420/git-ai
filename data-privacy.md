# Data privacy in this fork

This document describes behavior implemented by the `woud420/git-ai` source
tree. It does not make promises for services, accounts, editor marketplaces, or
deployments operated by anyone else.

## Repository collection is opt-in

Git AI processes attribution and agent-session events only for repositories
matched by `allowed_repositories`. An empty allowlist denies every repository.
Add a local path or remote URL glob explicitly:

```bash
git-ai config --add allowed_repositories <path-or-url>
```

`exclude_repositories` takes precedence over the allowlist. Opting a repository
in permits local collection; it does not by itself enable telemetry or select a
remote storage backend. The coding agent and Git may have their own independent
data and network behavior.

## Data kept locally

For an allowed repository, the current implementation can write:

- Working checkpoint JSON and file snapshots under
  `.git/ai/working_logs/<base-commit>/`.
- Authorship records to `~/.git-ai/internal/notes-db` when the production
  default `notes_backend.kind = sqlite` is in use. Local-origin rows are not
  uploaded by that backend.
- Transcript watermarks, derived metrics events, Bash provenance, and legacy
  queue state in databases under `~/.git-ai/internal/`; the exact stores and
  retention rules are listed in
  [`docs/contracts/persistence-model.md`](docs/contracts/persistence-model.md).
- User configuration in `~/.git-ai/config.json`. An API key stored there is
  masked in Git AI's serialized configuration output, but the file itself must
  still be protected as a credential.

Full transcript content remains in the coding agents' own transcript files in
the current streaming pipeline. Git AI reads those files and stores derived,
redacted events locally for allowed repositories. The agents' own storage and
retention policies are outside this fork's control.

## Authorship backend choices

The authorship backend is independent of the repository allowlist:

| `notes_backend.kind` | Authority | Sharing boundary |
| --- | --- | --- |
| `sqlite` (production default) | `~/.git-ai/internal/notes-db` | Local-origin records are not uploaded. Existing `refs/notes/ai` data can be read as a compatibility fallback and cached locally. |
| `git_notes` (opt-in) | `refs/notes/ai` in the repository | Records become available to others only when that Git notes ref is explicitly transferred. Anyone who receives the ref can read its authorship metadata. |
| `http` (opt-in) | The configured remote server | Writes are queued in the local notes database and sent only when `notes_backend.backend_url` and authentication are configured. The remote operator controls storage after receipt. |

Use `git-ai notes migrate --to <backend>` for an intentional backend change.
Do not treat the local SQLite cache used by the HTTP backend as the authority.

Authorship records contain line ranges plus agent, model, session, human Git
identity, and acceptance metadata. A record can also contain a prompt locator.
Review that metadata before exporting or pushing `refs/notes/ai`.

## Prompt and session sharing

`prompt_storage` defaults to `local`. The configuration resolver also accepts
two non-local modes, which should be treated as explicit permission to share:

- `local` keeps prompt sharing local.
- `notes` permits prompt data to be represented through Git notes after the
  implementation's redaction step; transferring the notes ref can share it.
- `default` permits prompt/CAS data to be queued for the configured
  `api_base_url`.

`exclude_prompts_in_repositories` forces matching repositories back to local
prompt storage. `include_prompts_in_repositories` and
`default_prompt_storage` can narrow a non-local mode. These settings do not
change the external coding agent's own transcript storage.

The current persistence model marks the legacy prompt/CAS queue as dormant.
That current implementation detail is not a safe reason to configure a
non-local mode: it remains an egress-capable contract and may be exercised by
compatible or future code paths.

## Network-capable features

Telemetry is off by default. The master `telemetry` setting must be `on` (or
the legacy `telemetry_oss` setting must be `on`) before built-in OSS
Sentry/PostHog events, metrics uploads, daemon-log uploads, or heartbeats can be
sent. API metrics and daemon logs also require login or an API key; daemon-log
upload additionally respects its feature flag.

`telemetry_enterprise_dsn` is an independent explicit opt-in. When configured,
errors can be sent to that DSN even while the master telemetry setting is off.
The endpoint operator, not this fork, controls received data.

Other actions that can make network requests include:

- Selecting `notes_backend.kind = http` with a backend URL and credentials.
- Selecting non-local `prompt_storage` and configuring or authenticating to an
  API endpoint (the current legacy CAS queue is documented as dormant).
- Running authenticated analytics commands such as `git-ai analyze`.
- Version checks and automatic updates. Their defaults depend on how the binary
  was built; use `disable_version_checks` and `disable_auto_updates` when those
  requests are not acceptable.

Configuration alone does not change what an external server retains. Before
enabling any network-capable mode, inspect the endpoint, authentication, and
operator policy that apply to that deployment.

## External-service boundary

This fork does not operate Git AI Cloud, personal dashboards, Teams or
Enterprise hosting, a Trust Center, or the `usegitai.com` API named by the
default `api_base_url`. Those are external offerings inherited as integration
points from the original codebase. Their availability, access controls,
retention, deletion, and privacy terms are not guarantees of this repository.

The same boundary applies to a self-hosted or custom endpoint: the organization
running it is the data controller. Pointing this client at that endpoint does
not make the service part of this fork.

## Inspect before enabling egress

Use `git-ai config` to inspect the effective user configuration. At minimum,
check `telemetry`, `telemetry_enterprise_dsn`, `notes_backend`,
`prompt_storage`, `api_base_url`, and the repository include/exclude lists.
Keep telemetry off, leave the production-default SQLite notes backend in place,
and keep prompt storage local when you require this client to avoid optional
Git AI service egress.
