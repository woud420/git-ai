# Agent sandbox socket permissions

Git AI leaves agent sandbox permissions unchanged by default. To let the hook
installer add the active daemon trace socket to supported agent configurations:

```sh
git-ai config set feature_flags.whitelist_agent_sandboxes true
git-ai install-hooks --dry-run --verbose
git-ai install-hooks
```

`GIT_AI_WHITELIST_AGENT_SANDBOXES=true` enables the same option for one command.
Only the configured trace socket is added; the control socket is not included.
This does not change repository collection permissions or the sandbox-aware
daemon startup and durable checkpoint outbox policies.

- **Codex on Unix:** the existing network proxy must already be enabled. Git AI
  adds an `allow` entry under `features.network_proxy.unix_sockets`, preserves
  domain rules, and refuses to replace an explicit socket denial. Enabling the
  proxy changes outbound network policy, so Git AI does not enable it for you.
- **Claude Code on macOS:** the sandbox must already be enabled. Git AI adds the
  socket to `sandbox.network.allowUnixSockets` without changing other settings.
- **Claude Code on Linux/WSL and agents on Windows:** this installer does not add
  a socket allowance. It reports the unsupported path-scoped permission instead
  of enabling broad Unix socket access.

Agent versions, selected permission profiles, and higher-priority managed or
project settings can still restrict access. The installer edits the user-level
configuration; it does not test connectivity from a running agent sandbox.

Re-run `install-hooks` with the option enabled after changing the daemon socket
path. It removes an older allowance only if Git AI recorded ownership and the
allowance is unchanged. Existing user allowances, including an allowance for the
same socket, remain user-owned.

`git-ai uninstall-hooks` removes recorded allowances even after the option is
disabled or hooks were manually removed. `--dry-run --verbose` previews cleanup.
Disabling the option by itself leaves previously installed permissions in place.
Ownership is stored beside each agent configuration in
`.git-ai-sandbox-socket.json`; retain that file for automatic cleanup. A failed
ownership write rolls back the config edit, but a process crash between the two
writes can leave an allowance requiring manual removal. Git AI treats an
unrecorded allowance as user-owned rather than deleting it speculatively.

Configuration contracts: [Codex permissions](https://learn.chatgpt.com/docs/permissions)
and [Claude sandbox settings](https://code.claude.com/docs/en/settings-reference#sandbox-network-allowunixsockets).
