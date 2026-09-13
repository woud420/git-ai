use super::{ExpectedLineExt, TestRepo, fs};

// =============================================================================
// Category A: Secondary file missing from authorship note
//
// Reproduction of fuzz_checkpoint_heavy_0:
// A multi-file commit includes fuzz_main.txt, fuzz_secondary_2.txt, and
// fuzz_secondary_3.txt — all with checkpointed edits — but the resulting
// authorship note only contains entries for some files, dropping others.
// =============================================================================

/// Multi-file commit where secondary file has AI checkpoint but is missing from note.
///
/// Models the fuzz_checkpoint_heavy_0 failure:
/// 1. Initial commit with AI on main file
/// 2. Selective commit of main file only (secondary stays dirty)
/// 3. Edit secondary files with checkpoints
/// 4. Commit all files together
/// 5. Note should include ALL files with attributed edits
#[test]
fn test_multifile_commit_secondary_file_missing_from_note() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit: AI edits on main file
    fs::write(&main_path, "AAA\nAAA\nAAA\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Edit both files, but only commit main
    fs::write(&main_path, "AAA\nAAA\nAAA\nBBB\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&sec_path, "CCC\nCCC\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // Only stage and commit main.txt — secondary stays dirty
    repo.git(&["add", "main.txt"]).unwrap();
    repo.commit("commit main only").unwrap();

    // Now commit everything (secondary.txt is still dirty from before)
    fs::write(&sec_path, "CCC\nCCC\nDDD\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("commit all files").unwrap();

    // Both files should have attribution
    let mut main_file = repo.filename("main.txt");
    main_file.assert_committed_lines(crate::lines![
        "AAA".ai(),
        "AAA".ai(),
        "AAA".ai(),
        "BBB".ai(),
    ]);

    let mut sec_file = repo.filename("secondary.txt");
    sec_file.assert_committed_lines(crate::lines!["CCC".ai(), "CCC".ai(), "DDD".ai(),]);
}

/// Simpler multi-file case: both files edited and committed in one shot.
#[test]
fn test_multifile_commit_both_files_attributed() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("other.txt");

    // Initial commit
    fs::write(&main_path, "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Edit both files with AI checkpoints
    fs::write(&main_path, "base\nnew-main\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&sec_path, "new-other\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "other.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("multi-file commit").unwrap();

    let mut main_file = repo.filename("main.txt");
    main_file.assert_committed_lines(crate::lines!["base".ai(), "new-main".ai()]);

    let mut sec_file = repo.filename("other.txt");
    sec_file.assert_committed_lines(crate::lines!["new-other".ai()]);
}

// =============================================================================
// Category E: File rename not tracked in authorship note
//
// Reproduction of fuzz_seed_3:
// After `git mv old.txt new.txt`, the authorship note for the commit still
// references the old filename. Blame on the new file finds no matching note
// entry, so all lines default to human.
// =============================================================================

/// Simple rename: AI-attributed file is renamed, note must reference new name.
#[test]
fn test_rename_file_note_tracks_new_name() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("original.txt");

    // Initial commit with AI content
    fs::write(&file_path, "ai-1\nai-2\nai-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "original.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let mut file = repo.filename("original.txt");
    file.assert_committed_lines(crate::lines!["ai-1".ai(), "ai-2".ai(), "ai-3".ai(),]);

    // Rename the file
    repo.git(&["mv", "original.txt", "renamed.txt"]).unwrap();
    repo.commit("rename file").unwrap();

    // Attribution should follow the rename
    let mut renamed = repo.filename("renamed.txt");
    renamed.assert_committed_lines(crate::lines!["ai-1".ai(), "ai-2".ai(), "ai-3".ai(),]);
}

/// Rename + edit in same commit: new content should be attributed to the new name.
#[test]
fn test_rename_and_edit_same_commit() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("original.txt");

    // Initial commit with AI content
    fs::write(&file_path, "ai-1\nai-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "original.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Rename and add new AI content
    repo.git(&["mv", "original.txt", "renamed.txt"]).unwrap();
    let renamed_path = repo.path().join("renamed.txt");
    fs::write(&renamed_path, "ai-1\nai-2\nnew-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "renamed.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("rename and edit").unwrap();

    let mut renamed = repo.filename("renamed.txt");
    renamed.assert_committed_lines(crate::lines!["ai-1".ai(), "ai-2".ai(), "new-ai".ai(),]);
}

// =============================================================================
// Category F: Secondary file missing from multi-file commit note
//
// Reproduction of fuzz_seed_4 and fuzz_checkpoint_heavy_0:
// A commit touches multiple files, all with AI checkpoints, but the resulting
// authorship note only contains entries for some files (typically fuzz_main.txt),
// dropping others entirely.
// =============================================================================

/// Two files checkpointed, committed together — both must appear in note.
#[test]
fn test_multi_file_both_in_note() {
    let repo = TestRepo::new();
    let file_a = repo.path().join("file_a.txt");
    let file_b = repo.path().join("file_b.txt");

    // Initial commit
    fs::write(&file_a, "a-init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Edit both files with AI checkpoints
    fs::write(&file_a, "a-init\na-new\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_a.txt"])
        .unwrap();
    fs::write(&file_b, "b-new-1\nb-new-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "file_b.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("multi-file").unwrap();

    let mut fa = repo.filename("file_a.txt");
    fa.assert_committed_lines(crate::lines!["a-init".ai(), "a-new".ai(),]);

    let mut fb = repo.filename("file_b.txt");
    fb.assert_committed_lines(crate::lines!["b-new-1".ai(), "b-new-2".ai(),]);
}

/// Three files: main + two secondaries. All have checkpoints. All must be in note.
/// Models fuzz_checkpoint_heavy_0 exactly.
#[test]
fn test_three_files_secondary_dropped_from_note() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec2_path = repo.path().join("secondary_2.txt");
    let sec3_path = repo.path().join("secondary_3.txt");

    // Initial commit on main
    fs::write(&main_path, "main-init\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Multiple edits and checkpoints on all files
    fs::write(&main_path, "main-init\nmain-ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    fs::write(&sec2_path, "sec2-line1\nsec2-line2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary_2.txt"])
        .unwrap();

    fs::write(&sec3_path, "sec3-line1\nsec3-line2\nsec3-line3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary_3.txt"])
        .unwrap();

    repo.git(&["add", "-A"]).unwrap();
    repo.commit("all three files").unwrap();

    let mut main = repo.filename("main.txt");
    main.assert_committed_lines(crate::lines!["main-init".ai(), "main-ai".ai(),]);

    let mut sec2 = repo.filename("secondary_2.txt");
    sec2.assert_committed_lines(crate::lines!["sec2-line1".ai(), "sec2-line2".ai(),]);

    let mut sec3 = repo.filename("secondary_3.txt");
    sec3.assert_committed_lines(crate::lines![
        "sec3-line1".ai(),
        "sec3-line2".ai(),
        "sec3-line3".ai(),
    ]);
}

/// Secondary file checkpointed BEFORE an intervening commit on another file.
/// The checkpoint's base_commit is now stale. On final commit, secondary is
/// dropped from the note because the working log base doesn't match HEAD.
///
/// This is the exact pattern from fuzz_checkpoint_heavy_0:
/// 1. Edit main + secondary, checkpoint both
/// 2. Commit ONLY main (selective-file-commit)
/// 3. More edits/checkpoints on main, more commits
/// 4. Commit everything — secondary's stale checkpoint is lost
#[test]
fn test_secondary_file_stale_checkpoint_across_commits() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let sec_path = repo.path().join("secondary.txt");

    // Initial commit
    fs::write(&main_path, "main\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Checkpoint BOTH files
    fs::write(&main_path, "main\nmain-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    fs::write(&sec_path, "sec-1\nsec-2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // Commit ONLY main — secondary stays dirty with stale checkpoint
    repo.git(&["add", "main.txt"]).unwrap();
    repo.commit("main only").unwrap();

    // More work on main (advances HEAD further from secondary's checkpoint base)
    fs::write(&main_path, "main\nmain-2\nmain-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();
    repo.git(&["add", "main.txt"]).unwrap();
    repo.commit("advance main again").unwrap();

    // Now commit everything — secondary's checkpoint was based on initial commit
    fs::write(&sec_path, "sec-1\nsec-2\nsec-3\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.commit("include secondary").unwrap();

    let mut sec = repo.filename("secondary.txt");
    sec.assert_committed_lines(crate::lines!["sec-1".ai(), "sec-2".ai(), "sec-3".ai(),]);
}
