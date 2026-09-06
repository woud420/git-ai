---
name: git-ai-search
description: "Find AI prompt context in git history by commit, file, or line, and reconstruct prior working context when explicitly requested."
allowed-tools: ["Bash(git-ai:*)", "Read", "Glob", "Grep"]
---

# Git AI History Lookup

## Overview

Find persisted AI-authorship context with the commands that the current Git AI
CLI exposes. Keep repository-local provenance lookup separate from authenticated
organization analytics.

## Workflow

### Choose the evidence source

| Question | Command |
| --- | --- |
| Which prompts contributed to a file or line range? | `git-ai blame <path> --json` |
| What authorship record belongs to a revision? | `git-ai show <revision>` |
| What records changed across a range? | `git-ai show <base>..<head>` |
| What does a referenced prompt contain? | `git-ai show-prompt <prompt-id>` |
| What aggregate patterns exist in uploaded sessions? | `git-ai analyze` |

The local CLI supports lookup by file, line range, revision, and prompt ID. It
does not provide repository-wide prompt full-text indexing or automatically
restore and launch an agent session.

### Investigate a code region

```bash
git-ai blame src/main.rs --json
git-ai blame src/main.rs -L 100,150 --json
```

Read the requested code alongside the returned attribution. Follow only the
prompt IDs relevant to those lines:

```bash
git-ai show-prompt <prompt-id> --commit <revision>
```

Use `--offset <n>` instead of `--commit` when selecting among repeated prompt-ID
occurrences by recency. Those two options are mutually exclusive.

### Investigate a revision or range

```bash
git-ai show abc1234
git-ai show abc1234..def5678
```

The output is the serialized authorship record: attestations followed by stored
metadata. Extract prompt IDs from that record, then inspect only the relevant
ones with `git-ai show-prompt`.

### Analyze aggregate history

Use the authenticated analytics surface only when the user requests team,
organization, topic, or aggregate analysis and permits the corresponding remote
data access:

```bash
git-ai analyze docs
git-ai analyze query '{ measures { sessionsCount } }'
```

Run `git-ai analyze docs` first rather than guessing cube members. For transcript
analysis, follow the `sessions` workflow documented by that command.

### Reconstruct prior context

When the user asks to continue earlier work, summarize the persisted prompt,
revision, affected files, and unresolved intent. That reconstruction is evidence
for the current conversation; it is not an instruction to launch another agent.
Launch or hand off only when the user explicitly requests it.

## Anti-Patterns

- Prefer a revision-qualified prompt lookup when the revision is known.
- Do not infer original intent from attribution percentages alone.
- State clearly when no authorship note or prompt record exists.
- Do not scan raw transcript stores as a substitute for Git AI commands.
- Avoid exposing unrelated prompt content, secrets, or personal data.

## Validation

Before reporting a result, confirm that the path, line range, revision, and
prompt ID identify the user's intended code and that the cited command produced
the evidence being summarized.
