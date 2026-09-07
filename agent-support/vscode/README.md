# git-ai Extension for VS Code and Cursor

The bundled extension connects VS Code and Cursor editor events to
[git-ai](https://github.com/woud420/git-ai#install-and-quick-start). It records
known-human checkpoints for evidenced editor saves and can display committed
authorship in the gutter. Depending on the editor version, agent edits come
from native hooks or high-confidence legacy event heuristics; this is not an
AI source-code detector.

## Support status

- VS Code 1.99.3 or newer, as declared by the extension package. VS Code
  1.109.3 and newer uses native Copilot hooks; eligible older versions use the
  extension's legacy Copilot event path.
- Cursor 1.7 or newer, as enforced by the git-ai installer.
- git-ai 1.0.23 or newer. This fork currently requires a source build because
  it has not published release artifacts.
- Node.js 20.18.1 or newer for building or testing the checked-in extension
  source.

## Install

Build and install the CLI from [this fork's source](../../README.md#install-and-quick-start).
The source installer runs `git-ai install-hooks`; when the `code` or `cursor`
CLI is available, that command installs extension ID `git-ai.git-ai-vscode`.

That extension is fetched from an externally operated Marketplace. It is not
a VSIX built from this checkout, and its publication, privacy policy, and
release cadence are outside this fork's guarantees. To inspect or package the
checked-in extension instead, follow the development commands below.

The installer also makes these editor-specific changes:

- VS Code: enables `chat.useHooks` and
  `github.copilot.chat.otel.dbSpanExporter.enabled` in eligible user settings.
- Cursor: writes git-ai `preToolUse` and `postToolUse` entries to
  `~/.cursor/hooks.json` while preserving unrelated hooks.

If automatic extension installation is unavailable, install
`git-ai.git-ai-vscode` from the
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=git-ai.git-ai-vscode)
or search for that ID in Cursor's extensions view. Restart the editor after
installation; in Cursor, restart Cursor rather than VS Code.

## Uninstall

While the CLI is still installed, remove managed hooks with:

```bash
git-ai uninstall-hooks --dry-run=false
```

This removes git-ai's Cursor hook entries, but it does not remove the extension
from either editor. Uninstall `git-ai.git-ai-vscode` through each editor's
Extensions view. The current VS Code uninstaller also does not restore the two
settings enabled during installation; review
`chat.useHooks` and `github.copilot.chat.otel.dbSpanExporter.enabled` manually
if you no longer want them.

## Data and network boundary

The extension invokes the local git-ai CLI with editor, file, session, and
checkpoint metadata. Collection still requires the repository to match
`allowed_repositories`. The Marketplace and any optional git-ai storage or
telemetry endpoint are separate operators; see the fork's
[data privacy guide](../../data-privacy.md) before enabling egress.

## Telemetry

The checked-in extension sends a `vscode_extension_startup` event to
`https://us.i.posthog.com` unless `telemetry_oss` is exactly `"off"` in
`~/.git-ai/config.json`. A missing setting is not an opt-out for this extension,
and it does not currently read the newer `telemetry` key used by the Rust
CLI/daemon. Disable it before starting VS Code or Cursor:

```json
{
  "telemetry_oss": "off"
}
```

The startup event includes the editor host, app name, URI scheme, extension
version, and the CLI-created pseudonymous distinct ID when available. The
PostHog service is externally operated; see the
[data privacy guide](../../data-privacy.md) for the separate CLI, storage, and
endpoint boundaries.

## Debug logging

Enable checkpoint toast messages in editor settings when diagnosing the
event heuristics:

```json
"gitai.enableCheckpointLogging": true
```

Use the messages to assess the effectiveness of the heuristics on your editor
version. A toast confirms that the extension emitted a checkpoint; it does not
turn an ambiguous edit into AI evidence.

## Attribution display

The `gitai.blameMode` setting controls attribution decorations:

- `line` (default) shows the current line's attribution in the gutter.
- `all` shows gutter decorations for every AI-attributed line in the file.
- `off` hides attribution gutter decorations.

Use **Git AI: Toggle Show AI Code** or `Cmd+Shift+A` on macOS
(`Ctrl+Shift+A` elsewhere) to switch modes. The choice is stored globally and
extension updates do not overwrite an explicit setting.

## AI tab tracking (experimental)

`gitai.experiments.aiTabTracking` is off by default. When enabled, it attempts
to recognize accepted tab completions and requires an editor restart. Add this
to `settings.json`:

```json
"gitai.experiments.aiTabTracking": true
```

## Development

From this directory, install dependencies and run the extension checks:

```bash
npm install
npm test
```

## License

[MIT](LICENSE.md)
