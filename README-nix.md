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

          # Add git-ai to system packages
          environment.systemPackages = [
            git-ai.packages.x86_64-linux.default
          ];
        }
      ];
    };
  };
}
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

cargo build
cargo test
cargo run -- --version
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
| `setGitAlias` | bool | `true` | Add git-ai to system PATH |

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
| `updateChannel` | null or channel name | `null` | Release/update channel |

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
