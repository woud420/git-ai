# Retired IDE extensions

## Integration inventory

The support baseline is pinned to `acunniffe-oai/git-ai` commit
`13ed6b12f2ba4f252daa26a5ea14d50c334ad7f0`:
[installer registry](https://github.com/acunniffe-oai/git-ai/blob/13ed6b12f2ba4f252daa26a5ea14d50c334ad7f0/src/mdm/agents/mod.rs)
and [preset registry](https://github.com/acunniffe-oai/git-ai/blob/13ed6b12f2ba4f252daa26a5ea14d50c334ad7f0/src/commands/checkpoint_agent/presets/mod.rs).

| Surface | Support |
| --- | --- |
| Standalone Git AI VS Code/Cursor extension | Retired |
| Git AI JetBrains/IntelliJ plugin | Retired |
| Git AI Visual Studio extension | Retired |
| Extension-only `ai_tab` checkpoint adapter | Retired |
| Claude Code, Codex, Cline, Cursor, GitHub Copilot, Amp, OpenCode, Pi, Gemini, Droid, Firebender, Windsurf | Retained native hooks/installers |
| Continue CLI, `agent-v1`, Cursor background agents | Retained checkpoint adapters; no dedicated installer added |
| `human`, `known_human`, `mock_ai`, `mock_known_human` | Retained boundary, evidence, and test APIs |

This adopts the pinned branch's integration scope, not its session-only data
model. This fork retains line attribution, transcripts, authorship backends,
and asynchronous Trace2 rewrite tracking. Historical decision documents describe
older implementations and do not override this inventory.

## Upgrade and uninstall

Upgrading the CLI stops automatic installation of the retired extensions,
including the Cursor and Windsurf extension extras. It does not uninstall an
existing editor extension. Close active agent sessions and remove each obsolete
extension explicitly before using the upgraded CLI:

1. In VS Code, VS Code Insiders, Cursor, Windsurf, and remote editor profiles,
   uninstall **Git AI** (`git-ai.git-ai-vscode`) through the Extensions panel.
   Alternatively, use that editor's CLI, for example
   `code --uninstall-extension git-ai.git-ai-vscode`; use `code-insiders`,
   `cursor`, or `windsurf` for the corresponding installation. Repeat for
   remote hosts, containers, and profiles where it was installed.
2. In JetBrains IDEs, open Settings/Preferences > Plugins > Installed and
   uninstall **Git AI** (`com.usegitai.plugins.jetbrains`); restart the IDE when prompted.
3. In Visual Studio, open Extensions > Manage Extensions > Installed and
   uninstall **git-ai** (`GitAiVS.A1B2C3D4-E5F6-7890-ABCD-EF1234567890`); close Visual Studio
   to complete removal. The old `--visual-studio-extension` installer option
   is no longer accepted.
4. Remove only Git AI's retired extension entries from your own extension
   recommendations, Settings Sync lists, and `devcontainer.json` extension
   lists. Remove `gitai.experiments.aiTabTracking`, `gitai.blameMode`, and
   `gitai.enableCheckpointLogging` settings if present. Preserve
   unrelated extensions and user settings. Do not remove native Copilot,
   Cline, Cursor, or other retained agent hooks. Keep shared settings such as
   `chat.useHooks`: native Copilot uses them too.
5. Run `git-ai install-hooks --dry-run --verbose` to preview native hooks,
   then `git-ai install-hooks` to refresh them. To remove retained managed
   agent hooks instead, preview `git-ai uninstall-hooks --dry-run --verbose`
   and apply with `git-ai uninstall-hooks`. These commands operate on managed
   hooks and do not uninstall retired editor packages.

Any custom hook that invokes `git-ai checkpoint ai_tab` must be removed from
its owning integration's configuration. The retired standalone extensions ran
listeners inside the editor rather than installing a separate `ai_tab` hook
file; uninstalling the package stops those listeners. Do not delete an entire
settings or hooks file to remove one managed reference.

## Capability and data compatibility

Extension-provided tab-completion attribution, editor attribution UI, call-stack
recognition, and automatic editor known-human capture are no longer supplied.
Native agents still provide checkpoint/transcript evidence. The explicit
`known_human` API remains available to integrations with human-input evidence;
`human` remains an untracked boundary and never substitutes for that evidence.
Edits without evidence remain unknown or untracked.

Existing authorship records and working logs remain readable without migration
or deletion. Legacy `AiTab` working-log and journal tags, plus `ai_tab` metric
aggregation, remain decoding compatibility only; the retired checkpoint preset
is rejected. Do not purge `~/.git-ai`, `.git/ai`, notes databases, or Git notes
as part of this upgrade.

Old extension binaries already installed on a machine retain their own behavior,
including telemetry. Removing source and release workflows cannot disable them
remotely. Uninstall them to stop their listeners and network clients; before
removal, legacy VS Code/IntelliJ clients require `"telemetry_oss": "off"` in
`~/.git-ai/config.json` to disable their built-in telemetry.

## Release and maintenance

CLI releases no longer build, test, update, or publish the retired packages.
Marketplace listings and previously published binaries are externally managed;
this change does not withdraw them. Native OpenCode/Pi/Amp support and shared
editor CLI, JSON settings, and Cline extension-manifest detection remain.
The removed editor-extension retry/list subprocess tests have no remaining
production caller; native editor CLI detection and retained hook lifecycle
coverage remain necessary.
