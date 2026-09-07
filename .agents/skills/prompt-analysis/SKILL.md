---
name: prompt-analysis
description: "Analyze prompt history, acceptance rates, and AI-assisted coding patterns in a repository when the user asks about prompting quality or outcomes."
allowed-tools: ["Bash(git-ai:*)", "Read", "Glob", "Grep", "Task"]
---

# Prompt Analysis

## Overview

Analyze uploaded Git AI session data with the current authenticated analytics
workflow. Use local authorship commands only for bounded file, line, revision, or
prompt lookups.

## Workflow

### Confirm scope and authorization

Clarify the repository, person or team, time range, and metric implied by the
question. Organization analytics can involve network access and data stored by
an external service, so do not start it when the user requested local-only work
or has not authorized the relevant access.

### Discover the analytics schema

Never guess measure or dimension names. Read the live reference first:

```bash
git-ai analyze docs
```

For a bounded aggregate query, use a documented cube shape:

```bash
git-ai analyze query '{ measures { sessionsCount } }'
```

Treat the server result as the source of truth for available metrics. Explain
filters, exclusions, and missing data in the final analysis.

### Pull sessions for transcript analysis

When the question requires classifying or grading individual conversations,
create a scratch database from an explicitly filtered slice:

```bash
DB=$(git-ai analyze sessions pull --since "last 30 days" --limit 100)
git-ai analyze sessions stats "$DB"
```

Use additional documented filters such as `--user`, `--agent`, or repository
filters when the question requires them. Keep the returned database path in a
task-specific variable and do not overwrite an unrelated analysis database.

### Define and persist a rubric

Add only the columns needed for the requested judgment:

```bash
git-ai analyze sessions exec "$DB" \
  "ALTER TABLE sessions ADD COLUMN clarity_score INTEGER"
git-ai analyze sessions exec "$DB" \
  "ALTER TABLE sessions ADD COLUMN clarity_reason TEXT"
```

Define categories and decision rules before grading. Each judgment should cite
specific evidence in the session record and distinguish observed facts from
interpretation.

### Iterate safely

```bash
git-ai analyze sessions reset "$DB"
git-ai analyze sessions next "$DB"
```

`sessions next` returns one session and advances the cursor. Analyze the supplied
record, then persist the result with `sessions exec`. Check progress with
`sessions stats`. If parallel workers are available and requested, ensure they
share the same rubric and use the command's atomic cursor rather than assigning
rows by assumption.

Example write-back:

```bash
git-ai analyze sessions exec "$DB" \
  "UPDATE sessions SET clarity_score = 4, clarity_reason = 'Specific goal and constraints' WHERE id = '<session-id>'"
```

Escape SQL values safely. Do not place secrets or unnecessary transcript text in
derived columns.

### Synthesize results

Query the enriched database for counts and comparisons:

```bash
git-ai analyze sessions exec "$DB" \
  "SELECT clarity_score, COUNT(*) FROM sessions GROUP BY clarity_score ORDER BY clarity_score"
```

Report the sample definition, sample size, missing values, rubric, distribution,
and representative evidence. Do not imply causation from a correlation such as
acceptance rate versus prompt style.

### Local provenance fallback

For a narrow code question that does not require remote analytics, use:

```bash
git-ai blame src/example.rs -L 10,30 --json
git-ai show <revision>
git-ai show-prompt <prompt-id> --commit <revision>
```

These commands inspect persisted repository authorship context; they are not a
replacement for aggregate organization metrics.

## Anti-Patterns

- Analyze only the people, repositories, and period the user authorized.
- Minimize quoted transcript content and redact credentials or personal data.
- Keep the scratch database local and remove it only when the user requests or
  the documented workflow makes cleanup explicit.
- State when incomplete uploads, missing notes, or selection bias limit the
  result.

## Validation

Before presenting findings, verify the live schema, the selected population and
time range, the cursor completion state, and the aggregate queries used in the
summary. Label subjective rubric judgments as interpretations.
