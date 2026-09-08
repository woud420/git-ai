/// Tests for intermediate-commit note integrity after rebase.
///
/// ## The Bug
///
/// The old rebase authorship rewriter had a
/// slow-path processing loop that seeds `cached_file_attestation_text` and
/// `existing_files` from the **full cumulative state of the last pre-rebase commit**
/// (all commits in the chain combined). When it writes the note for an *intermediate*
/// new commit K, it emits ALL entries in `cached_file_attestation_text` that appear in
/// `existing_files`, which includes files introduced by commits K+1, K+2, … (future
/// commits).
///
/// Concrete example from PR #967 (5-commit chain):
///   - Commit f70ab45e (early – daemon changes only): note shows `revert_hooks.rs`
///     attributed, but revert_hooks.rs was first introduced in a LATER commit
///     (f1fdede4). Every intermediate commit ended up with the same large set of
///     file attestations as the tip commit, and `accepted_lines` counts became
///     non-monotonic (earlier commits showed higher counts than later ones).
///
/// ## When the slow path fires
///
/// The fast path (`try_fast_path_rebase_note_remap_cached`) just copies original
/// notes verbatim (only updating `base_commit_sha`) and is correct. It only fires
/// when the AI-touched file blobs are *identical* between old and new commits. If
/// *any* tracked file's blob changes after rebasing (e.g. the upstream prepended a
/// header to a file the feature also modifies), the fast path is skipped and the
/// buggy slow path runs.
///
/// ## Setup pattern used to force the slow path
///
/// Each test creates a `shared.rs` file (with a proper trailing newline, committed via
/// `git_og` to avoid the no-trailing-newline issue with `set_contents`). The upstream
/// branch prepends a header to `shared.rs`. The feature branch then APPENDS to
/// `shared.rs` via `set_contents` (which writes content without a trailing newline –
/// that's fine because git can merge "prepend on upstream" with "append on feature"
/// even when they have different trailing-newline styles).
///
/// After rebasing, the shared.rs blob in each feature commit differs from its
/// pre-rebase counterpart, so `tracked_paths_match_for_commit_pairs` returns false
/// → fast path is bypassed → the buggy slow path runs.
///
/// ## Expected vs broken behaviour
///
/// | Commit | Expected note files                | Broken note files (current)          |
/// |--------|------------------------------------|--------------------------------------|
/// | A′     | shared.rs + module_a.rs            | shared.rs + module_a.rs + module_b.rs ← LEAK |
/// | B′     | shared.rs + module_a.rs + module_b.rs | correct (it is the tip)           |
///
/// These tests are intentionally written to **FAIL** with the current (buggy) code
/// and to **PASS** once the bug is fixed.
use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use crate::test_utils::session_line_count;
use git_ai::model::authorship_log_serialization::AuthorshipLog;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn total_accepted_lines(note: &str) -> u32 {
    let log = AuthorshipLog::deserialize_from_string(note)
        .expect("should parse authorship note as AuthorshipLog");
    // Count AI lines from attestations where hash starts with "s_" (sessions)
    session_line_count(&log)
}

fn files_in_note(note: &str) -> Vec<String> {
    let log = AuthorshipLog::deserialize_from_string(note)
        .expect("should parse authorship note as AuthorshipLog");
    log.attestations
        .iter()
        .map(|a| a.file_path.clone())
        .collect()
}

mod file_coverage;
mod line_attribution;
mod note_preservation;
