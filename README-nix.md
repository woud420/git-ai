# Installing git-ai with Nix

This project provides a Nix flake for easy installation on NixOS, nix-darwin, or any system using Home Manager or Nix profiles.

## Quick Start

Try without installing:
```bash
nix run github:woud420/git-ai -- --version
```

Install to user profile:
```bash
nix profile install github:woud420/git-ai
```

These commands evaluate this fork directly from source. The fork has not yet
published versioned release tags, so pin the flake input to a commit when you
need a reproducible installation.

## What's Included

The package provides three commands:

| Command | Description |
|---------|-------------|
| `git` | Routes through git-ai (tracks AI authorship) |
| `git-ai` | Direct git-ai commands |
| `git-og` | Bypasses git-ai, calls real git |

## Flake Outputs

```
packages.${system}.default   # Complete package with git wrapper
packages.${system}.minimal   # Without git symlink (for manual integration)
packages.${system}.unwrapped # Just the binary
devShells.${system}.default  # Development environment
nixosModules.default         # NixOS module
homeManagerModules.default   # Home Manager package, hooks, and config module
overlays.default             # Nixpkgs overlay
```

## Installation Methods

### 1. Home Manager with programs.git (Recommended)

The cleanest approach is to set git-ai as your git package and use the module for hooks.

Add the input to your flake:
```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    home-manager.url = "github:nix-community/home-manager";
    git-ai.url = "github:woud420/git-ai";
  };
}
```

In your Home Manager configuration:
```nix
{ inputs, system, ... }:

{
  imports = [ inputs.git-ai.homeManagerModules.default ];

  # Use git-ai as the git implementation
  programs.git = {
    enable = true;
    package = inputs.git-ai.packages.${system}.default;
    # ... your other git settings (signing, aliases, etc.)
  };

  # Enable git-ai hooks for IDE/agent integration
  programs.git-ai = {
    enable = true;
    installHooks = true;  # Runs git-ai install-hooks on activation
    settings.allowRepositories = [
      "https://github.com/myorg/*"
    ];
  };
}
```

Collection is opt-in. Leaving `settings.allowRepositories` null or empty denies
every repository, even when hooks are installed. Add only the local paths or
remote URL globs that Git AI should process.

This approach:
- Replaces the standard git with git-ai throughout your environment
- Installs IDE/agent hooks automatically
- Creates `~/.git-ai/config.json` with the correct git path
- Avoids package conflicts

### 2. nix-darwin with Home Manager

```nix
{
  inputs = {
    darwin.url = "github:lnl7/nix-darwin";
    home-manager.url = "github:nix-community/home-manager";
    git-ai.url = "github:woud420/git-ai";
  };

  outputs = { darwin, home-manager, git-ai, nixpkgs, ... }: {
    darwinConfigurations.myhost = darwin.lib.darwinSystem {
      system = "aarch64-darwin";
      modules = [
        home-manager.darwinModules.home-manager
        {
          home-manager.users.myuser = { pkgs, ... }: {
            imports = [ git-ai.homeManagerModules.default ];

            programs.git = {
              enable = true;
              package = git-ai.packages.${pkgs.system}.default;
            };

            programs.git-ai = {
              enable = true;
              installHooks = true;
              settings.allowRepositories = [
                "https://github.com/myorg/*"
              ];
            };
          };
        }
      ];
    };
  };
}
```

### 3. NixOS System-Wide

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    git-ai.url = "github:woud420/git-ai";
  };

  outputs = { nixpkgs, git-ai, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        git-ai.nixosModules.default
        {
          programs.git-ai = {
            enable = true;
            installHooks = true;
            settings.allowRepositories = [
              "https://github.com/myorg/*"
            ];
          };
        }
      ];
    };
  };
}
```

The module already installs its selected `package`. Its default is the complete
package with the `git` wrapper. To keep the system Git command and install only
`git-ai` plus `git-og`, select the executable `minimal` output instead:

```nix
programs.git-ai.package = git-ai.packages.x86_64-linux.minimal;
```

### 4. Direct Package (Standalone)

If not using Home Manager's `programs.git`, add the package directly:
```nix
{ inputs, pkgs, ... }:

{
  home.packages = [
    inputs.git-ai.packages.${pkgs.system}.default
  ];
}
```

**Note:** This may conflict if you also have `programs.git.enable = true`. Use the `minimal` package to avoid conflicts:
```nix
home.packages = [
  inputs.git-ai.packages.${pkgs.system}.minimal  # No git symlink
];
```

### 5. Using the Overlay

```nix
{
  nixpkgs.overlays = [ inputs.git-ai.overlays.default ];

  # Then use:
  home.packages = [ pkgs.git-ai ];
}
```

## Configuration ownership

Home Manager owns `~/.git-ai/config.json` as a link to generated Nix-store
content. Change `programs.git-ai.settings` and run `home-manager switch` (or
the NixOS/nix-darwin rebuild that includes Home Manager). Do not use
`git-ai config set` to edit this managed file.

The NixOS module without Home Manager behaves differently: when `installHooks`
is enabled, activation copies initial defaults into an absent or symlinked
config, producing a regular user-editable file. Later activations preserve an
existing regular file. With `installHooks = false`, that activation does not
create the config. Changing `settings.allowRepositories` or
`settings.excludeRepositories` in Nix therefore does **not** update an existing
regular file; removing an allowlist entry in Nix alone does not revoke its live
permission.

For that NixOS user-owned file, back up and inspect the existing config before
changing it. Run these commands as the affected user, substituting the intended
repository globs. `set` replaces the list; `--add` would retain old entries.
Supported unrelated settings are retained by the CLI:

```bash
git-ai config allowed_repositories
git-ai config exclude_repositories
git-ai config set allowed_repositories '["https://github.com/myorg/*"]'
git-ai config set exclude_repositories '["https://github.com/myorg/private-*"]'
```

To revoke collection for every repository in that file, explicitly clear the
allowlist and verify the result:

```bash
git-ai config set allowed_repositories '[]'
git-ai config allowed_repositories
```

The runtime accepts the older `allow_repositories` JSON key as an alias for
`allowed_repositories`; valid existing files do not need a forced migration.
The CLI writes the canonical spelling on its next config update. Do not keep
both spellings in the same JSON object.

## Uninstall

Nix owns the package and its declaration. `git-ai uninstall` cannot remove a
Nix-owned package or declaration, but it cleans up runtime hooks, Trace2
configuration, the daemon, and installer-owned state. While the binary is still
available, run this cleanup before removing the package:

```bash
git-ai uninstall --yes
```

The default retains local configuration and databases in `~/.git-ai`. If you
want to delete those too, use this alternative **instead of** the command above,
also before removing the package:

```bash
git-ai uninstall --yes --purge
```

Neither path removes repo-local `.git/ai` directories.

Editor extensions that require manual removal remain documented in their
integration READMEs. For a direct profile install, inspect the profile's
reported package name and remove it (substitute that name if it is not
`git-ai`):

```bash
nix profile list
nix profile remove git-ai
```

See the official [`nix profile remove` reference](https://nix.dev/manual/nix/stable/command-ref/new-cli/nix3-profile-remove)
for name and regular-expression selection.

For Home Manager, NixOS, or nix-darwin, remove the `programs.git-ai` block and
any `programs.git.package`, `home.packages`, or `environment.systemPackages`
entry that selects git-ai. Then rebuild with the command that owns the
configuration, for example:

```bash
home-manager switch
sudo nixos-rebuild switch
darwin-rebuild switch --flake .
```

## Development

Enter a development shell with Rust 1.93.0 or newer:
```bash
nix develop github:woud420/git-ai
```

Or clone and develop locally:
```bash
git clone https://github.com/woud420/git-ai
cd git-ai
nix develop

make build
make test
git-ai --version
```

## Local Flake Development

For developing from a local checkout:
```nix
{
  inputs.git-ai.url = "git+file:///path/to/git-ai";
}
```

## Module Options

### homeManagerModules.default

The Home Manager module installs the selected package and manages per-user hooks
and configuration.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enable` | bool | `false` | Enable git-ai hooks and config |
| `package` | package | flake default | Package installed for the user |
| `installHooks` | bool | `true` | Run `git-ai install-hooks` on activation |

### nixosModules.default

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enable` | bool | `false` | Enable git-ai |
| `package` | package | flake default | The git-ai package to use |
| `installHooks` | bool | `true` | Run `git-ai install-hooks` on activation |

### Shared settings

Both modules expose these under `programs.git-ai.settings`. A `null` value is
omitted from the generated JSON, so the runtime default applies.

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `gitPath` | null or string | `null` | Real Git executable; defaults to the Nixpkgs Git binary |
| `apiKey` | null or string | `null` | API key stored in the Nix store; prefer one of the indirect options |
| `apiKeyFile` | null or string | `null` | File read at shell startup to set `GIT_AI_API_KEY` |
| `apiKeyCommand` | null or string | `null` | Command run at shell startup to obtain `GIT_AI_API_KEY` |
| `promptStorage` | null or `default`, `notes`, or `local` | `null` | Prompt storage mode |
| `apiBaseUrl` | null or string | `null` | API base URL |
| `excludePromptsInRepositories` | null or list of strings | `null` | Remote URL globs that must not share prompts |
| `includePromptsInRepositories` | null or list of strings | `null` | Remote URL globs to which `promptStorage` applies |
| `defaultPromptStorage` | null or `default`, `notes`, or `local` | `null` | Storage mode outside the prompt include list |
| `allowRepositories` | null or list of strings | `null` (deny all) | Local path or remote URL globs opted into collection |
| `excludeRepositories` | null or list of strings | `null` | Collection exclusions; take precedence over the allowlist |
| `telemetryOss` | null or `on` or `off` | `null` | Legacy OSS telemetry setting |
| `telemetryEnterpriseDsn` | null or string | `null` | Custom telemetry endpoint |
| `disableVersionChecks` | null or bool | `null` | Disable version checks |
| `disableAutoUpdates` | null or bool | `null` | Disable automatic updates |
| `updateChannel` | null or `latest`, `next`, `enterprise-latest`, or `enterprise-next` | `null` | Release/update channel |

### Feature flags

Typed feature flags live under `programs.git-ai.settings.featureFlags`. Their
Nix default is `null`, which preserves the runtime's build-specific default.

| Option | Runtime default (debug / release) | Purpose |
|--------|-----------------------------------|---------|
| `featureFlags.authKeyring` | off / off | System keyring authentication |
| `featureFlags.transcriptStreaming` | on / on | Event-driven transcript streaming |
| `featureFlags.transcriptSweep` | on / on | Periodic discovery of missed transcript data |
| `featureFlags.checkpointDebugLog` | off / off | Detailed checkpoint debug logging |
| `featureFlags.bashCheckpointsV2` | off / off | Daemon-based Bash checkpoint flow |
| `featureFlags.daemonLogUpload` | on / on | Daemon log upload eligibility |
| `featureFlags.rewriteMetricsEvents` | on / off | Rewrite metrics event emission |
| `featureFlags.extraFlags` | `{}` | Forward-compatible snake_case boolean flags |

The declarations in [`flake.nix`](flake.nix) are the source of truth for option
types and descriptions. Unknown runtime flags can be supplied through
`featureFlags.extraFlags` until a typed option is added.

## Platforms

Supported systems:
- `x86_64-linux`
- `aarch64-linux`
- `x86_64-darwin`
- `aarch64-darwin`
