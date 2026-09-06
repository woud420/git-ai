# git-ai Plugin for OpenCode

This plugin connects [OpenCode](https://opencode.ai) tool events to the local
[git-ai](https://github.com/woud420/git-ai) checkpoint interface. It separates
pre-existing, unattributed changes from edits made by the active AI session;
it does not claim that the compatibility `human` checkpoint is evidence of
human authorship.

## Support status

The installer detects the `opencode` and `opencode2` commands. No OpenCode
version floor is pinned in this repository; the integration requires support
for the `tool.execute.before` and `tool.execute.after` plugin events. A missing
or failed hook never blocks OpenCode, so verify attribution after upgrades.

## Install

Build and install git-ai from [this fork's source](../../README.md#install-and-quick-start).
The source installer runs `git-ai install-hooks` and writes a managed plugin to:

- `~/.config/opencode/plugins/git-ai.ts`

The installer injects the absolute path of the installed git-ai binary. It
also removes the obsolete `~/.config/opencode/plugin/git-ai.ts` path (singular
`plugin`) so an older copy cannot shadow the managed plugin.

For local repository development, use the canonical Make command before
exercising the installer:

```bash
make build
git-ai install-hooks
```

## How it works

For recognized edit tools (`edit`, `write`, `patch`, `multiedit`, and
`apply_patch` variants) and Bash/shell tools, the plugin:

1. Sends a pre-tool checkpoint to establish an untracked boundary.
2. Sends a matching post-tool checkpoint to attribute the resulting delta to
   the AI session.

The payload passed to the local git-ai CLI includes the tool name, raw tool
input, session ID, tool-call ID, repository working directory, and extracted
file paths. Raw tool input can itself contain commands, patches, or source
fragments. The checkpoint subprocess has a 10-second timeout. Launch errors,
non-zero exits, and hook exceptions are swallowed so OpenCode can continue;
set `GIT_AI_OPENCODE_DEBUG=1` or `GIT_AI_DEBUG=1` before launching OpenCode to
log those failures.

OpenCode does not emit `known_human` checkpoints. That evidence-backed
category is reserved for editor integrations that can identify actual human
input.

## Uninstall

While the CLI is still available, remove both the managed current path and any
legacy singular-path copy with:

```bash
git-ai uninstall-hooks --dry-run=false
```

User-owned project-local OpenCode plugins are not managed by this global
installer and are left in place.

## Data and network boundary

The plugin does not make direct network requests. It sends its checkpoint
payload only to the local git-ai CLI; storage and optional egress then follow
the CLI configuration, repository allowlist, and
[fork privacy contract](../../data-privacy.md). OpenCode itself has independent
data and network behavior outside this integration's control.

## Development

From `agent-support/opencode`, install dependencies and type-check the template:

```bash
npm install
npm run type-check
```

## License

[Apache License 2.0](../../LICENSE)
