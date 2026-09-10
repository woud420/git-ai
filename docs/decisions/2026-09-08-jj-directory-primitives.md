# Shared Unix directory primitives for native jj (ENG-415)

Status: accepted — maintained low-level directory primitives used by native capture.

`unix_directory` exposes crate-private descriptor-relative directory opening and
an owned raw directory stream. It moves the existing outbox opener unchanged,
including `O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`. Each stream opens
`.` relative to the supplied directory, creating an independent file description
and offset. A duplicate descriptor would share offsets and could make a second
head scan start at EOF.

One `next_raw_name` call performs one `readdir` attempt and returns owned name
bytes. Dots, junk, EOF and errors are visible to the caller; EINTR is not retried
internally. The native capture can therefore count every call before filtering
and apply its own deadline. This helper itself does not bound an entire scan,
validate source paths or prove an atomic filesystem snapshot.

The outbox keeps a thin wrapper with its existing dot-only filtering and
`scan root` error mapping. Ownership, mode, ACL, link-count, capacity, locking,
collision, consumption and durability policies remain in their existing modules.
On successful `fdopendir`, stream ownership transfers to `closedir`; failed
construction retains the error and closes only the new descriptor. Caller-owned
descriptors remain open. Renaming a directory does not redirect an open stream;
path-currentness checks remain a later capture responsibility.

Linux/macOS errno handling is moved unchanged. Other Unix targets are not newly
qualified; Windows paths are unchanged. Darwin does not expose O_DIRECTORY in
F_GETFL, so tests verify directory/type/symlink behavior and portable descriptor
flags. Synthetic dirent bytes test non-UTF-8 preservation without assuming the
local filesystem accepts such filenames.

Ten default unit tests cover offsets, raw entry accounting, errors, ownership
and rename anchoring. A ten-second watchdog explicitly drives the ignored child
for FIFO/socket refusal and 128 descriptor-error cleanup cycles. Two outbox
characterization tests passed before extraction. The neutral tests then failed
on the missing API before implementation. Independent review and exact extraction
comparison found no remaining issue.

Final verification passed 137 top-level tests: ten neutral directory cases,
67 outbox unit tests, three daemon replay scenarios, twelve checkpoint journal
cases, twelve source/storage policies and 33 fork workflow policies. The ignored
watchdog child was explicitly invoked and passed. Fresh build, Rust 1.93
all-target lint and final formatting passed. Local runtime evidence is macOS;
Linux execution and Windows compilation remain new-head CI gates. Logs are
retained as `git-ai-jj-directory-*`.
