---
name: ask
description: "Use this when exploring a specific code file, symbol, or selection and you want the original AI-authorship context behind how or why it was written."
allowed-tools: ["Bash(git-ai:*)", "Read", "Glob", "Grep"]
---

# Ask

## Overview

Answer a question about a concrete code location using the authorship and prompt
records stored by Git AI. Attribute intent only when a stored prompt supports it.

## Workflow

### Resolve the code location

Use the narrowest location supplied by the user:

1. An editor selection supplies the path and line range directly.
2. An explicit path and range take precedence over inferred locations.
3. For a named symbol, find its definition and use that range.
4. For a path without a range, inspect the whole file.

If no file, selection, or identifiable symbol is available, ask the user to
provide one. Do not guess which code they meant.

### Retrieve authorship evidence

Start with machine-readable blame output for the selected lines:

```bash
git-ai blame src/example.rs -L 23,54 --json
```

Use the returned line attribution and prompt identifiers to distinguish AI,
known-human, and untracked changes. For a compact human-readable view with
prompt identifiers in the blame rows, use:

```bash
git-ai blame src/example.rs -L 23,54 --show-prompt
```

`--show-prompt` adds prompt identifiers to the human-readable blame rows; it
does not replace `show-prompt` when the stored prompt content is needed.

If a revision matters, inspect its complete authorship record too:

```bash
git-ai show <revision>
```

Retrieve a referenced prompt by ID:

```bash
git-ai show-prompt <prompt-id> --commit <revision>
```

When the same prompt ID appears more than once and no revision is known, select
an occurrence by recency instead:

```bash
git-ai show-prompt <prompt-id> --offset 1
```

`--commit` and `--offset` are mutually exclusive. The command prints the stored
prompt record as JSON; do not invent unsupported output flags.

### Answer

- Lead with the direct answer.
- Cite the stored request or response that explains the choice, without exposing
  unrelated prompt content.
- State the relevant revision, file, lines, and prompt ID when useful.
- Use first-person author voice only when the prompt record actually establishes
  that intent.
- If no prompt record is available, say so and give an objective code-based
  explanation instead.

## Anti-Patterns

- Do not read raw databases, agent logs, or transcript directories.
- Do not treat line attribution alone as evidence of design intent.
- Do not claim that human or untracked code was authored by an AI agent.
- Do not launch another coding agent unless the user explicitly asks for that
  separate action.

## Validation

Before answering, confirm that the inspected path and range match the user's
reference and that every claim about original intent is supported by the prompt
record you retrieved.
