# Bounded metadata file reads for native jj (ENG-415)

Status: accepted — maintained Unix file-read primitive used by native capture.

`regular_file::read_regular_at(parent, basename, per_file_maximum, &mut budget)`
reads one regular file relative to an already-open Unix directory. It returns
owned bytes. The dependency-neutral helper reuses the outbox's existing
`openat` primitive verbatim, including `O_RDONLY | O_NONBLOCK | O_NOFOLLOW |
O_CLOEXEC`. The outbox now re-exports that function; its ownership, permissions,
ACL, link-count, collision, locking and durability policies are unchanged.

This is an input-reading primitive. It does not discover a repository, bind a
registered source, read Git objects, run Git/jj, verify native content hashes,
claim an atomic filesystem snapshot, or enable jj attribution. The caller still
owns those joins and the directory chain leading to the supplied descriptor.

## Bounds and sampling

The basename must be one nonempty component of at most 255 bytes, excluding dot,
parent, slash and NUL. Other Unix filename bytes are preserved. Before opening,
the helper checks the named entry without following links. After opening it
requires a regular file with the same device/inode and reported size. Files that
exceed the per-file payload cap are rejected before payload allocation.

`MetadataReadBudget` supplies one absolute deadline, aggregate byte allowance and
file-attempt allowance for the whole batch. A file reserves its reported size
plus one byte for an EOF/growth probe before fallible allocation. Failed reads,
late results and changed files do not refund that reservation. Reads use bounded
chunks and check the same deadline around every syscall and interrupted retry.
Attempt permits also bound missing files and zero-length work. Counters represent
conservative read capacity, not physical disk/page activity or allocator overhead.

Early EOF, growth, replacement, or observed timestamp/size changes reject the
result. A final descriptor stamp and relative-name check reject visible changes.
An ancestor rename cannot redirect the captured directory handle; reading that
original directory can still succeed. The caller must recheck its captured
ancestor edges if it needs the directory's current pathname to remain bound.

The deadline is cooperative: filesystem calls can finish after it, at which point
the result is rejected. Equal metadata samples do not rule out intervening changes
or ABA. Native operation/view hashes and before/after mutable checkout/head byte
samples remain required by the integrated reader.

## Platform qualification

Non-Unix targets return `UnsupportedPlatform` before metadata access and have no
path-based fallback. This does not qualify a Windows observer. The public suite
has ten common Unix cases, one Linux-only real non-UTF-8 filename case, and a
Windows unsupported case. This Mac's filesystem rejected creation of the invalid
UTF-8 filename fixture with EILSEQ; raw basename byte preservation is separately
tested before filesystem access on Unix. Linux and Windows runtime qualification
remain CI gates.

## Verification

Public TestRepo tests failed on the missing module before implementation.
The local public GREEN passed all ten applicable tests. Thirteen private tests
passed, including a watchdog that explicitly runs an otherwise ignored child.
The child qualifies visible/raced-in FIFOs, sockets and 128 descriptor error paths.
Deterministic tests cover replacement before/after open, same-inode changes,
growth/truncation, EOF, read errors, interrupted retries and deadline expiry.
The public API exposes no injection hook; a private statically dispatched reader
allows tests to choose exact interleavings.

The moved syscall body and the sole outbox re-export/removal were compared
exactly. Independent source review found no actionable issue. A fresh build, Rust 1.93 all-target lint and formatting passed. All 148
top-level focused tests passed: ten public reader cases, thirteen private cases,
65 outbox unit cases, three daemon outbox-replay scenarios, twelve checkpoint
journal cases, twelve source/storage policies and 33 fork workflow policies.
The private child helper additionally passed when invoked by its watchdog.
The replay scenarios verify downtime recovery into attribution, deduplication
and quarantine for disallowed repositories.

```sh
rtk gmake test CARGO_TEST_ARGS='--test integration' TEST_FILTER=regular_file TEST_THREADS=2
rtk gmake test CARGO_TEST_ARGS='--lib' TEST_FILTER=regular_file TEST_THREADS=2
rtk gmake test CARGO_TEST_ARGS='--lib' TEST_FILTER=checkpoint_outbox TEST_THREADS=2
```
