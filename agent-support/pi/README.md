# Pi support

## Support status

This fork ships a managed Pi extension. `git-ai install-hooks` detects the
`pi` executable, a global `~/.pi` directory, or a repository-local `.pi`
directory; it does not enforce a Pi version floor.

The installer writes the extension to:

- `~/.pi/agent/extensions/git-ai.ts`

The generated file contains the absolute path of the installed `git-ai`
binary. A later `git-ai install-hooks --dry-run=false` may replace that managed
file to update the integration.

## Checkpoint behavior and data boundary

The extension runs the local `git-ai` CLI with checkpoint JSON on standard
input:

```bash
git-ai checkpoint pi --hook-input stdin
```

For built-in `edit` and `write` tools, it checkpoints before and after a
successful edit. Payloads can include the Pi session path, session ID, latest
assistant model, working directory, raw and canonical tool names, tool-call
ID, absolute file paths, dirty file contents, and tool input. The post-edit
payload also includes the tool result (`content`, `details`, and error state).

Bash commands use snapshot-based `before_command` and `after_command`
checkpoints. The extension emits the after checkpoint only when the command
succeeds. Tools that are neither built in nor configured as mutating do not
produce edit checkpoints.

The extension does not make direct network requests. It starts only the local
`git-ai` CLI; repository allowlisting, local storage, and any optional service
egress are controlled by git-ai. See [Data privacy](../../data-privacy.md) for
the fork-wide policy.

Session-reading and checkpoint process errors are swallowed rather than
surfaced as extension errors. Each event handler still waits for its checkpoint
subprocess to close, and there is no extension-level timeout.

## Override file

The optional override file is user-owned:

- `~/.pi/agent/git-ai.override.json`

`install-hooks` does not create or rewrite it. An override entry replaces the
built-in policy for the same raw tool name; invalid entries are ignored.

### Override contract

```json
{
  "version": 1,
  "tools": {
    "edit": {
      "kind": "mutating",
      "canonical": "edit",
      "filepath_fields": ["path"]
    },
    "bash": {
      "kind": "ignore"
    }
  }
}
```

Rules:

- The tool key is the raw Pi tool name.
- `kind: "mutating"` requires a `canonical` value of `edit`, `write`,
  `replace`, or `rename`, plus top-level `filepath_fields` used to extract
  touched paths.
- `kind: "ignore"` disables tracking for that raw tool.
- A missing override file means the built-in policies are used unchanged.

### Built-in defaults

Without an override file, the extension treats these tools as mutating:

- `edit` -> canonical `edit`, filepath field `path`
- `write` -> canonical `write`, filepath field `path`

### Example: add Serena mutators

```json
{
  "version": 1,
  "tools": {
    "serena_replace_symbol_body": {
      "kind": "mutating",
      "canonical": "replace",
      "filepath_fields": ["relative_path"]
    },
    "serena_insert_after_symbol": {
      "kind": "mutating",
      "canonical": "edit",
      "filepath_fields": ["relative_path"]
    },
    "serena_insert_before_symbol": {
      "kind": "mutating",
      "canonical": "edit",
      "filepath_fields": ["relative_path"]
    },
    "serena_replace_content": {
      "kind": "mutating",
      "canonical": "replace",
      "filepath_fields": ["relative_path"]
    },
    "serena_rename_symbol": {
      "kind": "mutating",
      "canonical": "rename",
      "filepath_fields": ["relative_path"]
    }
  }
}
```

## Uninstall

Preview and then remove the managed extension:

```bash
git-ai uninstall-hooks
git-ai uninstall-hooks --dry-run=false
```

Uninstall removes `~/.pi/agent/extensions/git-ai.ts`. The user-owned
`~/.pi/agent/git-ai.override.json` file is left in place; delete it separately
only if you no longer want the customization.

## Troubleshooting

Check the generated extension:

```bash
grep -n "checkpoint', 'pi'\|git-ai.override.json" ~/.pi/agent/extensions/git-ai.ts
```

Check tracked checkpoints in a repository:

```bash
cat .git/ai/working_logs/*/checkpoints.jsonl
```

Inspect the latest commit through the configured authorship backend:

```bash
git-ai show HEAD
```

Raw `git notes --ref=ai show HEAD` inspection applies only when the opt-in
`git_notes` backend is authoritative, or when diagnosing its compatibility
fallback. It will not show SQLite-default records that have not been exported.

## License

The managed extension is distributed with git-ai under the repository's
Apache License 2.0. See [`LICENSE`](../../LICENSE).
