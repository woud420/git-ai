# Machine-Readable CLI Output

The structured outputs external tooling may depend on. Anything not listed
here is human-facing text and may change freely. Additive JSON evolution
only: new optional fields are fine; renaming/removing fields or changing
types is a breaking change and needs a deprecation note in the release notes.

| Command | Shape source | Notes |
|---|---|---|
| `git-ai diff --json` | `DiffJson` / `FileDiffJson` (`operations/commands/diff.rs:86-141`) | per-file `annotations` map prompt-hash → line ranges, `diff`, `base_content`; optional fields use `skip_serializing_if` for forward compat |
| `git-ai status --json` | `StatusOutput` (`operations/commands/status.rs:18-34`) | `stats` + optional `checkpoints`; `--diff-only` narrows |
| `git-ai stats --json` | `CheckpointLineStats` (`model/working_log.rs`) | additions/deletions incl. `_sloc` variants |
| `git-ai usage --json` | `operations/commands/usage.rs` | |
| `git ai fetch-notes --json` | `FetchNotesJsonOutput` (`operations/commands/fetch_notes.rs`) | `remote`, `status` (`fetched`/`warmed`/error), optional `error` |
| Blame porcelain | `operations/commands/blame.rs` | line-oriented; author field carries the AI agent identity for AI lines |

Related wire contracts: upload envelopes (`model/api_types.rs` —
`DaemonLogsUploadRequest` v1, CAS/bundle/notes requests) are governed by the
server contract docs, not by this file; the notes HTTP wire contract is
`notes-backend-spec.md`.

The authorship note format itself — the most important machine-readable
surface — is specified in `specs/git_ai_standard_v3.0.0.md` and summarized in
`checkpoint-interface.md` §Note format.
