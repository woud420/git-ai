# Machine-Readable CLI Output

The structured outputs external tooling may depend on. Anything not listed
here is human-facing text and may change freely. Additive JSON evolution
only: new optional fields are fine; renaming/removing fields or changing
types is a breaking change and needs a deprecation note in the release notes.

| Command | Shape source | Notes |
|---|---|---|
| `git-ai diff --json` | `DiffJson` / `FileDiffJson` (`src/model/diff_json.rs`) | per-file `annotations` map prompt-hash → line ranges, `diff`, `base_content`; optional fields use `skip_serializing_if` for forward compatibility |
| `git-ai blame --json <file>` | `JsonBlameOutput` (`src/operations/commands/blame/json_output.rs`) | top-level `lines`, `prompts`, and `metadata`; referenced prompt records add `other_files` and `commits` |
| `git-ai status --json` | `StatusOutput` (`src/operations/commands/status.rs`) | `stats` + optional `checkpoints`; `--diff-only` narrows |
| `git-ai stats --json` | `CheckpointLineStats` (`src/model/working_log.rs`) | additions/deletions including `_sloc` variants |
| `git-ai usage --json` | `UsageJsonOutput` (`src/operations/commands/usage.rs`) | local activity totals plus per-repository summaries |
| `git ai fetch-notes --json` | `FetchNotesJsonOutput` (`src/operations/commands/fetch_notes.rs`) | `remote`, `status` (`fetched`/`warmed`/error), optional `error` |
| Blame porcelain (`--porcelain` / `--line-porcelain`) | `src/operations/commands/blame/porcelain.rs` | line-oriented; author field carries the AI agent identity for AI lines |

Related wire contracts: upload envelopes (`src/model/api_types.rs` —
`DaemonLogsUploadRequest` v1, CAS/bundle/notes requests) are governed by the
server contract docs, not by this file; the notes HTTP wire contract is
`notes-backend-spec.md`.

The authorship note format itself — the most important machine-readable
surface — is specified in `specs/git_ai_standard_v3.0.0.md` and summarized in
`checkpoint-interface.md` §Note format.
