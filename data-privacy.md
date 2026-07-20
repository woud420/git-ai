# Data Privacy

## Collection is opt-in per repository

Git AI collects nothing by default. Attribution and prompt collection run only in repositories you explicitly list in the `allowed_repositories` setting (`git-ai config --add allowed_repositories <path-or-url>`); every other repository is left untouched — no working logs, no notes, no prompt capture.

## OSS Mode

If you install Git AI open source and don't login **no code, prompts, or agent usage data is ever sent to Git AI**. Git AI runs entirely on your machine and writes attribution data into your local git repository and prompts to a local SQLite.

Telemetry is **off by default**. If you want to help us improve the tool you can opt in to error and exception telemetry with `git-ai config set telemetry on`, and turn it back off (or redirect it to your own endpoint via `telemetry_enterprise_dsn`) at any time. See [configuration options](https://usegitai.com/docs/cli/configuration#configuration-options) for details.

## Data

### Local-only data

- **Prompts** are captured only in repositories listed in `allowed_repositories` and are stored locally on the developer's laptop (`prompt_storage` defaults to `local`). They are never shared with teammates or Git AI unless you explicitly change `prompt_storage`.

### Attribution data

By default attribution notes are stored in a local SQLite database on your machine (`notes_backend.kind = sqlite`). If you opt into the `git_notes` backend (or export via `git-ai notes migrate --to git-notes`), attribution is written to git notes and is readable by anyone with repo access:

- Model, agent, and accepted-rate percentages
- Which lines are AI-generated
- Git profile (name, email) of the person who steered each prompt

### Telemetry

- **Error & exception telemetry** — off by default; only shared with Git AI if you opt in via `git-ai config set telemetry on`. You can redirect it to your own endpoint instead. See [configuration options](https://usegitai.com/docs/cli/configuration#configuration-options).

---

## Git AI Cloud (Personal Dashboards)

If you opt in to a personal dashboard, the following is uploaded to Git AI Cloud:

- **Agent usage telemetry** — cross-agent telemetry for every tool use, MCP call, skill invocation, interruption, error, and token-usage event, along with prompts and agent responses, used for your personal analytics
- **Personal Agent usage data** - % AI, # of Parallel agents and other stats on the dashboard are visible only to you unless you share them with others. 
- **SCM profile metadata** from GitHub, Bitbucket, or GitLab

See our [Cloud Privacy Policy](https://usegitai.com/privacy-policy) for details.

---

## Git AI for Teams and Enterprise

Teams and Enterprise deployments store additional data in the team instance:

- **Employee identity** — names, emails, and GitHub/Bitbucket/GitLab team membership
- **Agent usage telemetry** — cross-agent telemetry for every tool use, MCP call, skill invocation, interruption, error, and token-usage event
- **Full prompts** uploaded to the Git AI prompt store
  - Best-effort stripping of secrets and PII by default
  - You can apply your own filters for enhanced detection
  - Write-only: prompts are saved to your team's instance, but developers cannot read unless explicitly granted permission. 
- **Full agent sessions** can be reviewed, summarized, and made readable to developers
- **SCM PR data** from GitHub, Bitbucket, and GitLab
  - PR metadata: description, opener, reviewer, status
  - PR diffs — processed for computing % AI code, but not stored
- **Error & exception telemetry** — off by default; opt-in, or redirected to your own endpoint. See [configuration options](https://usegitai.com/docs/cli/configuration#configuration-options).

For more information, see our [Trust Center](https://trust.usegitai.com).

In [self-hosted deployments](https://github.com/git-ai-project/self-hosted), all data is sent only to your team's Git AI instance.
