# Standalone send-pack notes export

Git AI can export authorship notes after this successful native Git invocation:

```text
git [-C <source-worktree>] send-pack <absolute-destination-path> <full-source-oid>:refs/heads/<branch>
```

Enable `feature_flags.send_pack_notes_sync` in the Git AI config used by the
daemon. The flag defaults to false in debug and release builds. The source
repository must allow collection and use the `git_notes` backend; SQLite and
HTTP backends do not participate in this profile.

The opt-in authorizes sending the **entire current `refs/notes/ai` ref**, which
can include metadata for commits outside the branch sent by the original
command. It is not a per-commit metadata filter.

The family worker runs a separate, asynchronous `send-pack` for
`refs/notes/ai:refs/notes/ai` after observing native success. This uses the
literal destination rather than applying `url.*.insteadOf`,
`url.*.pushInsteadOf`, or named-remote configuration. Receiver hooks and
normal fast-forward checks remain active. There is no force, automatic
notes merge, or retry. Missing notes, a divergent remote notes ref, a receiver
rejection, and transport failure leave the original Git result unchanged;
the daemon logs a best-effort warning.

This operation exports the notes available when the worker processes it.
It does not reconstruct an operation-time notes snapshot. Notes can arrive
after the original Git process exits. The existing notes-transport timeout
bounds the separate operation, and its Git process count is independent of
commit and file count. Ingestion does not read reflogs for this command.

Other forms retain native behavior without the additional export: dry-run,
force, mirror, atomic, stdin, multiple refspecs, deletion, shorthand refs,
named remotes, relative destinations, network URLs, and global options other
than `-C`. A send-pack child of ordinary push does not initiate a second
export. Ordinary push keeps its existing notes-sync behavior.

Validation lives in `tests/send_pack_notes_sync.rs`, transport analyzer
unit tests, and `tests_send_pack.rs` in the daemon module.
