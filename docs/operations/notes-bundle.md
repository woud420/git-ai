# Export selected notes in a companion bundle

`git ai notes bundle <output.bundle> <full-commit-id>...` exports authorship
notes for explicitly selected commits. Run it separately from native
`git bundle`; the code bundle and its refspecs remain under your control.

```sh
git bundle create code.bundle main
git ai notes bundle notes.bundle <full-commit-id>

# In a receiving repository using the same object format:
git fetch /path/to/code.bundle main:refs/heads/imported
git fetch /path/to/notes.bundle refs/notes/ai:refs/notes/ai
```

The companion contains one parentless `refs/notes/ai` commit with only the
selected available notes. It has no prerequisites and does not contain the
source code commits. Fetch or otherwise obtain those commits separately.
Native `git bundle verify` and `git bundle list-heads` can inspect the
companion. SHA-1 and SHA-256 repositories are supported; their bundles cannot
be mixed. Exporting an existing SHA-256 note does not establish that every
Git AI workflow supports that object format.

The recipient fetch is explicit. An existing divergent `refs/notes/ai` is
rejected by ordinary Git fast-forward rules. Do not force it to replace
unrelated notes. Fetch into a separate notes ref for inspection when needed;
note reconciliation remains a separate operation. Git AI's SQLite reader
uses its existing Git-notes fallback and cache behavior, with local records
remaining authoritative.

## Initial support contract

- The caller supplies 1–32 full, non-zero commit IDs from the source
  repository. Branch names, ranges and object IDs for trees or blobs are
  rejected. Duplicate IDs are deduplicated after validating the input count.
- Repository collection must be enabled. The configured `git_notes` or
  `sqlite` backend supplies the notes through the existing batch API. HTTP
  export is currently unsupported and does not make a backend request.
- Every exported note must parse as `authorship/3.0.0`, name its selected
  commit and be no larger than 1 MiB. This is an export-size limit checked
  after the existing notes API reads the content; it is not a memory limit
  on those backend reads.
- Missing notes are omitted and counted in the result. If all are missing,
  the command fails without an artifact. Missing metadata is not inferred.
- The output must be a new file, not stdout. Existing files and symlinks are
  not overwritten, including a destination created during export. The file
  is published only after the complete bundle is written and synced.
- Repository and command-config environment overrides, including `GIT_DIR`,
  `GIT_COMMON_DIR`, `GIT_OBJECT_DIRECTORY`, `GIT_INDEX_FILE`,
  `GIT_CONFIG_COUNT` and `GIT_CONFIG_PARAMETERS`, are rejected. Invoke from
  a normal repository or linked-worktree directory without those overrides.

The command creates an isolated temporary bare repository with no template
or active hooks. It leaves source refs, files, index and working logs alone.
SQLite reads retain the existing cache backfill on a Git-notes fallback; the
command neither replaces authoritative rows nor materializes a source notes
ref. No automatic export or Trace2 ingestion work is added.

Notes may contain prompt and session metadata. The caller's explicit commit
selection determines what is included; the command does not redact the
selected records or automatically send the bundle anywhere.

## Verification

`tests/notes_bundle.rs` covers native bundle verification and fetch, line-level
attribution, unrelated-note exclusion, both local backends, SHA-256 note
transport, linked worktrees, native external-command invocation, collection
opt-out, invalid notes and selections, destination preservation, pending
work, inherited Git environment and hook isolation. It measures the same
number of internal Git processes for one and eight selected commits: nine
with GitNotes and six with SQLite-only notes. Git's own internal packing
subprocesses and the existing bounded config-lock retries are not included
in those normal-path counts.

See the [official Git bundle documentation](https://git-scm.com/docs/git-bundle)
for the native bundle format and prerequisite rules.
