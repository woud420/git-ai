use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::config::AuthorConfig;
use git_ai::model::attribution_tracker::Attribution;
use git_ai::model::working_log::{CheckpointKind, WorkingLogEntry};
use std::fs;

fn configure_diff_settings(repo: &TestRepo, settings: &[(&str, &str)]) {
    for (key, value) in settings {
        repo.git_og(&["config", key, value])
            .unwrap_or_else(|err| panic!("setting {key}={value} should succeed: {err}"));
    }
}

fn run_simple_additions_with_diff_settings(settings: &[(&str, &str)]) {
    let repo = TestRepo::new();
    configure_diff_settings(&repo, settings);

    let mut file = repo.filename("test.txt");
    file.set_contents(crate::lines!["Base line 1", "Base line 2"]);
    repo.stage_all_and_commit("Base commit").unwrap();

    file.insert_at(
        2,
        crate::lines!["NEW LINEs From Claude!".ai(), "Hello".ai(), "World".ai(),],
    );
    repo.stage_all_and_commit("AI additions").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "Base line 1".human(),
        "Base line 2".ai(),
        "NEW LINEs From Claude!".ai(),
        "Hello".ai(),
        "World".ai(),
    ]);
}

/// Helper: assert every SessionRecord.human_author in the note for `sha` contains the email.
fn assert_session_authors_have_email(repo: &TestRepo, sha: &str) {
    let log = repo.require_authorship_log(sha);
    assert!(
        !log.metadata.sessions.is_empty(),
        "commit {} should have sessions metadata",
        &sha[..8]
    );
    for (id, record) in &log.metadata.sessions {
        let author = record
            .human_author
            .as_deref()
            .unwrap_or_else(|| panic!("session {} should have human_author", id));
        assert_eq!(
            author, "Test User <test@example.com>",
            "session {} human_author should be full git identity",
            id
        );
    }
}

/// Helper: assert every HumanRecord.author in the note for `sha` contains the email.
fn assert_human_records_have_email(repo: &TestRepo, sha: &str) {
    let log = repo.require_authorship_log(sha);
    assert!(
        !log.metadata.humans.is_empty(),
        "commit {} should have humans metadata",
        &sha[..8]
    );
    for (id, record) in &log.metadata.humans {
        assert_eq!(
            record.author, "Test User <test@example.com>",
            "human record {} author should be full git identity",
            id
        );
    }
}

mod checkpoint_order;
mod deletions;

mod line_attribution;
mod multiple_sessions;
mod partial_staging;

mod readme_rewrite;

/// Regression test: known-human checkpoint must store the full git identity
/// ("Name <email>") in the HumanRecord, not just the name.
///
/// The test harness configures user.name = "Test User" and
/// user.email = "test@example.com", so the expected author field is
/// "Test User <test@example.com>".
#[test]
fn test_known_human_record_includes_email() {
    let repo = TestRepo::new();

    let file_path = repo.path().join("app.go");

    // AI writes the initial file
    repo.git_ai(&["checkpoint", "human", "app.go"]).unwrap();
    fs::write(&file_path, "func main() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "app.go"]).unwrap();
    repo.stage_all_and_commit("AI commit").unwrap();

    // Human edits the file
    fs::write(&file_path, "func main() {}\nfunc helper() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "app.go"])
        .unwrap();
    repo.stage_all_and_commit("Human commit").unwrap();

    let mut file = repo.filename("app.go");
    file.assert_committed_lines(crate::lines![
        "func main() {}".ai(),
        "func helper() {}".human(),
    ]);

    // Verify the HumanRecord has the full identity with email
    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = repo.require_authorship_log(&sha);
    assert!(
        !log.metadata.humans.is_empty(),
        "should have humans metadata"
    );
    for record in log.metadata.humans.values() {
        assert!(
            record.author.contains('<') && record.author.contains('>'),
            "HumanRecord.author should include email in angle brackets, got: {:?}",
            record.author
        );
        assert_eq!(
            record.author, "Test User <test@example.com>",
            "HumanRecord.author should be the full git identity"
        );
    }
}

#[test]
fn test_session_record_human_author_includes_email() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("main.rs");

    repo.git_ai(&["checkpoint", "human", "main.rs"]).unwrap();
    fs::write(&file_path, "fn main() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.rs"]).unwrap();
    repo.stage_all_and_commit("AI commit").unwrap();

    let mut file = repo.filename("main.rs");
    file.assert_committed_lines(crate::lines!["fn main() {}".ai()]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = repo.require_authorship_log(&sha);
    assert!(
        !log.metadata.sessions.is_empty(),
        "should have sessions metadata"
    );
    for record in log.metadata.sessions.values() {
        let author = record
            .human_author
            .as_deref()
            .expect("human_author should be set");
        assert_eq!(
            author, "Test User <test@example.com>",
            "SessionRecord.human_author should be the full git identity"
        );
    }
}

#[test]
fn test_author_config_cli_set_get_unset() {
    let repo = TestRepo::new();

    repo.git_ai(&["config", "set", "author.name", "Config User"])
        .unwrap();
    repo.git_ai(&["config", "set", "author.email", "config@example.com"])
        .unwrap();

    let name = repo.git_ai(&["config", "author.name"]).unwrap();
    assert_eq!(name.trim(), "\"Config User\"");

    let author = repo.git_ai(&["config", "author"]).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(author.trim()).expect("author config should be JSON");
    assert_eq!(value["name"], "Config User");
    assert_eq!(value["email"], "config@example.com");

    repo.git_ai(&["config", "unset", "author.name"]).unwrap();
    let author = repo.git_ai(&["config", "author"]).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(author.trim()).expect("author config should be JSON");
    assert!(value.get("name").is_none());
    assert_eq!(value["email"], "config@example.com");

    repo.git_ai(&["config", "unset", "author"]).unwrap();
    let author = repo.git_ai(&["config", "author"]).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(author.trim()).expect("author config should be JSON");
    assert_eq!(value.as_object().unwrap().len(), 0);
}

#[test]
fn test_author_config_overrides_session_and_known_human_records() {
    let mut repo = TestRepo::new();
    repo.patch_git_ai_config(|patch| {
        patch.author = Some(AuthorConfig {
            name: Some("Config User".to_string()),
            email: Some("config@example.com".to_string()),
        });
    });

    let file_path = repo.path().join("author_config.rs");
    repo.git_ai(&["checkpoint", "human", "author_config.rs"])
        .unwrap();
    fs::write(&file_path, "fn ai() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "author_config.rs"])
        .unwrap();
    repo.stage_all_and_commit("AI commit with author config")
        .unwrap();

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = repo.require_authorship_log(&sha);
    assert!(!log.metadata.sessions.is_empty());
    for session in log.metadata.sessions.values() {
        assert_eq!(
            session.human_author.as_deref(),
            Some("Config User <config@example.com>")
        );
    }

    fs::write(&file_path, "fn ai() {}\nfn human() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "author_config.rs"])
        .unwrap();
    repo.stage_all_and_commit("Known human commit with author config")
        .unwrap();

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let log = repo.require_authorship_log(&sha);
    assert!(!log.metadata.humans.is_empty());
    for human in log.metadata.humans.values() {
        assert_eq!(human.author, "Config User <config@example.com>");
    }
}

#[test]
fn test_author_config_partial_overrides_fall_back_to_git_committer_identity() {
    let mut name_repo = TestRepo::new();
    name_repo.patch_git_ai_config(|patch| {
        patch.author = Some(AuthorConfig {
            name: Some("Config Name".to_string()),
            email: None,
        });
    });
    let file_path = name_repo.path().join("partial_name.rs");
    name_repo
        .git_ai(&["checkpoint", "human", "partial_name.rs"])
        .unwrap();
    fs::write(&file_path, "fn ai() {}\n").unwrap();
    name_repo
        .git_ai(&["checkpoint", "mock_ai", "partial_name.rs"])
        .unwrap();
    name_repo
        .stage_all_and_commit("AI commit with author name override")
        .unwrap();
    let sha = name_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let log = name_repo.require_authorship_log(&sha);
    for session in log.metadata.sessions.values() {
        assert_eq!(
            session.human_author.as_deref(),
            Some("Config Name <test@example.com>")
        );
    }

    let mut email_repo = TestRepo::new();
    email_repo.patch_git_ai_config(|patch| {
        patch.author = Some(AuthorConfig {
            name: None,
            email: Some("configured-email@example.com".to_string()),
        });
    });
    let file_path = email_repo.path().join("partial_email.rs");
    email_repo
        .git_ai(&["checkpoint", "human", "partial_email.rs"])
        .unwrap();
    fs::write(&file_path, "fn ai() {}\n").unwrap();
    email_repo
        .git_ai(&["checkpoint", "mock_ai", "partial_email.rs"])
        .unwrap();
    email_repo
        .stage_all_and_commit("AI commit with author email override")
        .unwrap();
    let sha = email_repo
        .git(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let log = email_repo.require_authorship_log(&sha);
    for session in log.metadata.sessions.values() {
        assert_eq!(
            session.human_author.as_deref(),
            Some("Test User <configured-email@example.com>")
        );
    }
}

/// Verify that SessionRecord.human_author includes email after checkout carryover.
/// Exercises daemon.rs working log carryover path (checkout_hooks → restore_working_log_carryover).
#[test]
fn test_checkout_carryover_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("work.txt");

    fs::write(repo.path().join("README.md"), "init\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    repo.git(&["branch", "feature"]).unwrap();

    // Create AI checkpoint on main (uncommitted)
    repo.git_ai(&["checkpoint", "human", "work.txt"]).unwrap();
    fs::write(&file_path, "AI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "work.txt"]).unwrap();

    // Checkout feature — working log carries over
    repo.git(&["checkout", "feature"]).unwrap();

    // Commit on feature branch
    repo.stage_all_and_commit("commit on feature").unwrap();

    let mut file = repo.filename("work.txt");
    file.assert_committed_lines(crate::lines!["AI line".ai()]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

/// Verify that SessionRecord.human_author includes email after `git switch` carryover.
/// Exercises daemon.rs switch_hooks → restore_working_log_carryover path.
#[test]
fn test_switch_carryover_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("work.txt");

    fs::write(repo.path().join("README.md"), "init\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    repo.git(&["branch", "feature"]).unwrap();

    // Create AI checkpoint on main (uncommitted)
    repo.git_ai(&["checkpoint", "human", "work.txt"]).unwrap();
    fs::write(&file_path, "AI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "work.txt"]).unwrap();

    // Switch to feature — working log carries over
    repo.git(&["switch", "feature"]).unwrap();

    // Commit on feature branch
    repo.stage_all_and_commit("commit on feature").unwrap();

    let mut file = repo.filename("work.txt");
    file.assert_committed_lines(crate::lines!["AI line".ai()]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

/// Verify that SessionRecord.human_author includes email after rebase rewrites the note.
/// Exercises daemon.rs apply_rewrite_prerequisites → post_commit path.
#[test]
fn test_rebase_rewrite_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("code.rs");

    // Base commit
    fs::write(&file_path, "fn base() {}\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // AI commit on top
    repo.git_ai(&["checkpoint", "human", "code.rs"]).unwrap();
    fs::write(&file_path, "fn base() {}\nfn ai() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "code.rs"]).unwrap();
    repo.stage_all_and_commit("ai commit").unwrap();

    let mut file = repo.filename("code.rs");
    file.assert_committed_lines(crate::lines![
        "fn base() {}".unattributed_human(),
        "fn ai() {}".ai(),
    ]);

    // Create a new base commit on a side branch to rebase onto
    repo.git(&["checkout", "-b", "new-base", "HEAD~1"]).unwrap();
    fs::write(repo.path().join("other.txt"), "other\n").unwrap();
    repo.stage_all_and_commit("new base commit").unwrap();
    let new_base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Go back to the AI commit's branch and rebase
    repo.git(&["checkout", "-"]).unwrap();
    repo.git(&["rebase", &new_base]).unwrap();

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

/// Verify that HumanRecord.author includes email after rebase rewrites the note.
#[test]
fn test_rebase_rewrite_preserves_author_email_in_human_record() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("code.rs");

    // Base commit
    fs::write(&file_path, "fn base() {}\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // Known-human commit on top
    fs::write(&file_path, "fn base() {}\nfn human() {}\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "code.rs"])
        .unwrap();
    repo.stage_all_and_commit("human commit").unwrap();

    let mut file = repo.filename("code.rs");
    file.assert_committed_lines(crate::lines![
        "fn base() {}".unattributed_human(),
        "fn human() {}".human(),
    ]);

    // Create a new base commit on a side branch
    repo.git(&["checkout", "-b", "new-base", "HEAD~1"]).unwrap();
    fs::write(repo.path().join("other.txt"), "other\n").unwrap();
    repo.stage_all_and_commit("new base commit").unwrap();
    let new_base = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Go back and rebase
    repo.git(&["checkout", "-"]).unwrap();
    repo.git(&["rebase", &new_base]).unwrap();

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_human_records_have_email(&repo, &sha);
}

/// Verify that `git-ai status` implicit checkpoint flows through to email in SessionRecord.
/// Exercises status.rs → checkpoint::run → post_commit path.
#[test]
fn test_status_checkpoint_preserves_author_email_in_session() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("app.py");

    // Base commit
    fs::write(&file_path, "print('hello')\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // AI edits
    repo.git_ai(&["checkpoint", "human", "app.py"]).unwrap();
    fs::write(&file_path, "print('hello')\nprint('ai')\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "app.py"]).unwrap();

    // Run git-ai status (triggers implicit human checkpoint internally)
    let _ = repo.git_ai(&["status", "--json"]);

    // Commit after status
    repo.stage_all_and_commit("post-status commit").unwrap();

    let mut file = repo.filename("app.py");
    file.assert_committed_lines(crate::lines![
        "print('hello')".unattributed_human(),
        "print('ai')".ai(),
    ]);

    let sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_session_authors_have_email(&repo, &sha);
}

#[test]
fn test_simple_additions_with_base_commit_and_custom_diff_config() {
    run_simple_additions_with_diff_settings(&[
        ("diff.wordregex", r"\w+|[^[:space:]]+"),
        ("diff.mnemonicprefix", "true"),
        ("diff.renames", "copies"),
        ("diff.noprefix", "true"),
    ]);
}

#[test]
fn test_simple_additions_with_diff_noprefix_enabled() {
    run_simple_additions_with_diff_settings(&[("diff.noprefix", "true")]);
}

#[test]
fn test_simple_additions_with_diff_mnemonicprefix_enabled() {
    run_simple_additions_with_diff_settings(&[("diff.mnemonicprefix", "true")]);
}

#[test]
fn test_simple_additions_with_diff_renames_copies() {
    run_simple_additions_with_diff_settings(&[("diff.renames", "copies")]);
}

#[test]
fn test_simple_additions_with_diff_relative_enabled() {
    run_simple_additions_with_diff_settings(&[("diff.relative", "true")]);
}

#[test]
fn test_simple_additions_with_custom_diff_prefixes() {
    run_simple_additions_with_diff_settings(&[
        ("diff.srcPrefix", "SRC/"),
        ("diff.dstPrefix", "DST/"),
    ]);
}

#[test]
fn test_simple_additions_with_diff_algorithm_histogram() {
    run_simple_additions_with_diff_settings(&[("diff.algorithm", "histogram")]);
}

#[test]
fn test_simple_additions_with_diff_indent_heuristic_disabled() {
    run_simple_additions_with_diff_settings(&[("diff.indentHeuristic", "false")]);
}

#[test]
fn test_simple_additions_with_diff_inter_hunk_context() {
    run_simple_additions_with_diff_settings(&[("diff.interHunkContext", "8")]);
}

#[test]
fn test_simple_additions_with_color_diff_always() {
    run_simple_additions_with_diff_settings(&[("color.diff", "always"), ("color.ui", "always")]);
}

/// Regression test for issue #356
/// When AI edits multiple files in the same session, but they are committed
/// in separate batches, the second batch loses AI attribution.
/// See: https://github.com/git-ai-project/git-ai/issues/356
#[test]
fn test_multi_file_batch_commits_preserve_attribution() {
    // This test reproduces the exact scenario from issue #356:
    // 1. AI edits two files (file_a.txt and file_b.txt)
    // 2. User commits file_a.txt first -> AI attribution correct ✓
    // 3. User commits file_b.txt second -> AI attribution should be preserved
    use std::fs;

    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    // AI creates two new files in the same session
    let file_a_path = repo.path().join("file_a.txt");
    let file_b_path = repo.path().join("file_b.txt");

    fs::write(
        &file_a_path,
        "AI content for file A\nLine 2 from AI\nLine 3 from AI\n",
    )
    .unwrap();
    fs::write(
        &file_b_path,
        "AI content for file B\nLine 2 from AI\nLine 3 from AI\n",
    )
    .unwrap();

    // Single AI checkpoint covers both files (same AI session)
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    // First commit: only file_a.txt
    repo.git(&["add", "file_a.txt"]).unwrap();
    repo.commit("Add file A").unwrap();

    // Second commit: file_b.txt (this is where attribution is lost in issue #356)
    repo.git(&["add", "file_b.txt"]).unwrap();
    repo.commit("Add file B").unwrap();

    // Verify file_a.txt has correct AI attribution (this works)
    let mut file_a = repo.filename("file_a.txt");
    file_a.assert_lines_and_blame(crate::lines![
        "AI content for file A".ai(),
        "Line 2 from AI".ai(),
        "Line 3 from AI".ai(),
    ]);

    // Verify file_b.txt ALSO has correct AI attribution (this fails in issue #356)
    let mut file_b = repo.filename("file_b.txt");
    file_b.assert_lines_and_blame(crate::lines![
        "AI content for file B".ai(),
        "Line 2 from AI".ai(),
        "Line 3 from AI".ai(),
    ]);
}

/// Additional test for issue #356 with modifications instead of new files
#[test]
fn test_multi_file_batch_commits_modifications() {
    // Similar to above, but with modifications to existing files
    use std::fs;

    let repo = TestRepo::new();

    // Create initial files (human-authored)
    let file_a_path = repo.path().join("file_a.txt");
    let file_b_path = repo.path().join("file_b.txt");

    fs::write(&file_a_path, "Original content A\n").unwrap();
    fs::write(&file_b_path, "Original content B\n").unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial commit with both files")
        .unwrap();

    // AI modifies both files in the same session
    fs::write(&file_a_path, "Original content A\nAI added line A\n").unwrap();
    fs::write(&file_b_path, "Original content B\nAI added line B\n").unwrap();

    // Single AI checkpoint covers both modifications
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    // First commit: only file_a.txt
    repo.git(&["add", "file_a.txt"]).unwrap();
    repo.commit("Modify file A").unwrap();

    // Second commit: file_b.txt
    repo.git(&["add", "file_b.txt"]).unwrap();
    repo.commit("Modify file B").unwrap();

    // Verify both files have correct AI attribution
    let mut file_a = repo.filename("file_a.txt");
    file_a.assert_lines_and_blame(crate::lines![
        "Original content A".human(),
        "AI added line A".ai(),
    ]);

    let mut file_b = repo.filename("file_b.txt");
    file_b.assert_lines_and_blame(crate::lines![
        "Original content B".human(),
        "AI added line B".ai(), // This fails in issue #356 - shows as human
    ]);
}

#[test]
fn test_ai_edits_file_with_spaces_in_filename() {
    // Test that AI authorship tracking works correctly for files with spaces in the filename
    // This is a potential edge case that could fail if paths aren't properly quoted
    use std::fs;

    let repo = TestRepo::new();
    let file_path = repo.path().join("my test file.txt");

    // Initial commit: Create file with spaces in name
    fs::write(&file_path, "Line 1\nLine 2\nLine 3\n").unwrap();

    repo.git_ai(&["checkpoint"]).unwrap();
    repo.stage_all_and_commit("Initial commit with spaced filename")
        .unwrap();

    // AI adds new lines to the file
    fs::write(&file_path, "Line 1\nLine 2\nAI Line 1\nAI Line 2\nLine 3\n").unwrap();

    // Mark the AI-authored content with mock_ai checkpoint
    repo.git_ai(&["checkpoint", "mock_ai", "my test file.txt"])
        .unwrap();

    repo.stage_all_and_commit("AI adds lines to file with spaces")
        .unwrap();

    // Verify line-by-line attribution
    let mut file = repo.filename("my test file.txt");
    file.assert_lines_and_blame(crate::lines![
        "Line 1".human(),
        "Line 2".human(),
        "AI Line 1".ai(),
        "AI Line 2".ai(),
        "Line 3".human(),
    ]);
}

/// Reproduces fuzz_chaos_99: multi-file commit followed by soft-reset-recommit.
/// The secondary file's attribution must survive the reset+recommit cycle.
#[test]
fn test_soft_reset_recommit_preserves_secondary_file_attribution() {
    let repo = TestRepo::new();
    let main_path = repo.path().join("main.txt");
    let secondary_path = repo.path().join("secondary.txt");

    // Initial commit with untracked content
    fs::write(&main_path, "base\n").unwrap();
    fs::write(&secondary_path, "base\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    // Edit secondary file with multiple checkpoints (like the fuzzer does)
    // KnownHuman edit
    repo.git_ai(&["checkpoint", "human", "secondary.txt"])
        .unwrap();
    fs::write(&secondary_path, "base\nHH1\nHH2\nHH3\nHH4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "secondary.txt"])
        .unwrap();

    // AI append
    repo.git_ai(&["checkpoint", "human", "secondary.txt"])
        .unwrap();
    fs::write(&secondary_path, "base\nHH1\nHH2\nHH3\nHH4\nAI1\nAI2\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // AI prepend (shifts existing lines down)
    repo.git_ai(&["checkpoint", "human", "secondary.txt"])
        .unwrap();
    fs::write(
        &secondary_path,
        "P1\nP2\nP3\nP4\nbase\nHH1\nHH2\nHH3\nHH4\nAI1\nAI2\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "secondary.txt"])
        .unwrap();

    // Also edit main file
    repo.git_ai(&["checkpoint", "human", "main.txt"]).unwrap();
    fs::write(&main_path, "base\nmain_ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Commit both files
    repo.stage_all_and_commit("commit with both files").unwrap();

    // Verify attribution before reset
    let mut secondary = repo.filename("secondary.txt");
    secondary.assert_committed_lines(crate::lines![
        "P1".ai(),
        "P2".ai(),
        "P3".ai(),
        "P4".ai(),
        "base".unattributed_human(),
        "HH1".human(),
        "HH2".human(),
        "HH3".human(),
        "HH4".human(),
        "AI1".ai(),
        "AI2".ai(),
    ]);

    // Now do soft-reset-recommit: undo the commit, edit only main.txt, recommit
    repo.git(&["reset", "--soft", "HEAD~1"]).unwrap();

    // Edit main.txt further and checkpoint
    repo.git_ai(&["checkpoint", "human", "main.txt"]).unwrap();
    fs::write(&main_path, "base\nmain_ai\nextra\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "main.txt"]).unwrap();

    // Recommit everything
    repo.stage_all_and_commit("recommit after soft reset")
        .unwrap();

    // Secondary file's attribution should be preserved through the reset+recommit
    secondary.assert_committed_lines(crate::lines![
        "P1".ai(),
        "P2".ai(),
        "P3".ai(),
        "P4".ai(),
        "base".unattributed_human(),
        "HH1".human(),
        "HH2".human(),
        "HH3".human(),
        "HH4".human(),
        "AI1".ai(),
        "AI2".ai(),
    ]);
}

/// Reproduces the fuzz_chaos_99 bug: multiple checkpoints on the same file where a later
/// prepend checkpoint should preserve prior AI/KnownHuman attribution for shifted lines.
#[test]
fn test_multi_checkpoint_prepend_preserves_attribution() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Step 1: Initial content with KnownHuman
    let content1 = "AAAA\nBBBB\nCCCC\nDDDD\n";
    fs::write(&file_path, content1).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Step 2: Append AI lines
    let content2 = "AAAA\nBBBB\nCCCC\nDDDD\nEEEE\nFFFF\n";
    fs::write(&file_path, content2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Step 3: Prepend AI lines (this should preserve lines 1-6 attribution shifted to 9-14)
    // Pre-edit "human" snapshot
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let content3 =
        "1111\n2222\n3333\n4444\n5555\n6666\n7777\n8888\nAAAA\nBBBB\nCCCC\nDDDD\nEEEE\nFFFF\n";
    fs::write(&file_path, content3).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Commit
    repo.stage_all_and_commit("multi checkpoint test").unwrap();

    // Assert: lines 1-8 are AI (prepended), lines 9-12 are KnownHuman (shifted from original),
    // lines 13-14 are AI (shifted from step 2's append)
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines![
        "1111".ai(),
        "2222".ai(),
        "3333".ai(),
        "4444".ai(),
        "5555".ai(),
        "6666".ai(),
        "7777".ai(),
        "8888".ai(),
        "AAAA".human(), // KnownHuman shifted
        "BBBB".human(), // KnownHuman shifted
        "CCCC".human(), // KnownHuman shifted
        "DDDD".human(), // KnownHuman shifted
        "EEEE".ai(),    // AI shifted
        "FFFF".ai(),    // AI shifted
    ]);
}

/// Reproduces exact fuzz_chaos_99 pattern: 4 rapid edits (KnownHuman append, AI append,
/// KnownHuman ReplaceRandom, AI Prepend) where the final prepend must preserve all 8 lines.
#[test]
fn test_burst_edits_prepend_preserves_all_lines() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Start with some base content (simulates file before the burst)
    fs::write(&file_path, "X1\nX2\nX3\nX4\n").unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // Edit 1: KnownHuman Append 4 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(&file_path, "X1\nX2\nX3\nX4\nH1\nH2\nH3\nH4\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 2: AI Append 6 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(
        &file_path,
        "X1\nX2\nX3\nX4\nH1\nH2\nH3\nH4\nA1\nA2\nA3\nA4\nA5\nA6\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Edit 3: KnownHuman ReplaceRandom 8 lines (replace lines at positions 1-8)
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(
        &file_path,
        "R1\nR2\nR3\nR4\nR5\nR6\nR7\nR8\nA1\nA2\nA3\nA4\nA5\nA6\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 4: AI Prepend 8 lines - ALL 8 must be AI
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    fs::write(
        &file_path,
        "P1\nP2\nP3\nP4\nP5\nP6\nP7\nP8\nR1\nR2\nR3\nR4\nR5\nR6\nR7\nR8\nA1\nA2\nA3\nA4\nA5\nA6\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Commit
    repo.stage_all_and_commit("burst commit").unwrap();

    // Assert: ALL 8 prepended lines are AI, R1-R8 are KnownHuman, A1-A6 are AI
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines![
        "P1".ai(),
        "P2".ai(),
        "P3".ai(),
        "P4".ai(),
        "P5".ai(),
        "P6".ai(),
        "P7".ai(),
        "P8".ai(),
        "R1".human(),
        "R2".human(),
        "R3".human(),
        "R4".human(),
        "R5".human(),
        "R6".human(),
        "R7".human(),
        "R8".human(),
        "A1".ai(),
        "A2".ai(),
        "A3".ai(),
        "A4".ai(),
        "A5".ai(),
        "A6".ai(),
    ]);
}

/// Same as above but with single multi-byte Unicode chars per line (like the fuzzer uses).
/// The fuzzer allocates one char per step; when it exhausts ASCII, it uses U+0100+.
#[test]
fn test_burst_edits_prepend_multibyte_chars() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("test.txt");

    // Use multi-byte Unicode chars (2-3 bytes each in UTF-8)
    // These simulate what the fuzzer produces at steps 100+
    let base = "\u{0100}\n\u{0101}\n\u{0102}\n\u{0103}\n"; // Ā ā Ă ă
    fs::write(&file_path, base).unwrap();
    repo.stage_all_and_commit("base").unwrap();

    // Edit 1: KnownHuman Append 4 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit1 = "\u{0100}\n\u{0101}\n\u{0102}\n\u{0103}\n\u{0110}\n\u{0111}\n\u{0112}\n\u{0113}\n";
    fs::write(&file_path, edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 2: AI Append 6 lines
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit2 = "\u{0100}\n\u{0101}\n\u{0102}\n\u{0103}\n\u{0110}\n\u{0111}\n\u{0112}\n\u{0113}\n\u{0120}\n\u{0121}\n\u{0122}\n\u{0123}\n\u{0124}\n\u{0125}\n";
    fs::write(&file_path, edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Edit 3: KnownHuman ReplaceRandom 8 lines (replace first 8)
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit3 = "\u{0130}\n\u{0131}\n\u{0132}\n\u{0133}\n\u{0134}\n\u{0135}\n\u{0136}\n\u{0137}\n\u{0120}\n\u{0121}\n\u{0122}\n\u{0123}\n\u{0124}\n\u{0125}\n";
    fs::write(&file_path, edit3).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "test.txt"])
        .unwrap();

    // Edit 4: AI Prepend 8 lines - ALL 8 must be AI
    repo.git_ai(&["checkpoint", "human", "test.txt"]).unwrap();
    let edit4 = "\u{0140}\n\u{0141}\n\u{0142}\n\u{0143}\n\u{0144}\n\u{0145}\n\u{0146}\n\u{0147}\n\u{0130}\n\u{0131}\n\u{0132}\n\u{0133}\n\u{0134}\n\u{0135}\n\u{0136}\n\u{0137}\n\u{0120}\n\u{0121}\n\u{0122}\n\u{0123}\n\u{0124}\n\u{0125}\n";
    fs::write(&file_path, edit4).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "test.txt"]).unwrap();

    // Commit
    repo.stage_all_and_commit("burst commit").unwrap();

    // Assert: ALL 8 prepended lines are AI, next 8 are KnownHuman, last 6 are AI
    let mut file = repo.filename("test.txt");
    file.assert_committed_lines(crate::lines![
        "\u{0140}".ai(),
        "\u{0141}".ai(),
        "\u{0142}".ai(),
        "\u{0143}".ai(),
        "\u{0144}".ai(),
        "\u{0145}".ai(),
        "\u{0146}".ai(),
        "\u{0147}".ai(),
        "\u{0130}".human(),
        "\u{0131}".human(),
        "\u{0132}".human(),
        "\u{0133}".human(),
        "\u{0134}".human(),
        "\u{0135}".human(),
        "\u{0136}".human(),
        "\u{0137}".human(),
        "\u{0120}".ai(),
        "\u{0121}".ai(),
        "\u{0122}".ai(),
        "\u{0123}".ai(),
        "\u{0124}".ai(),
        "\u{0125}".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_known_human_record_includes_email,
    test_session_record_human_author_includes_email,
    test_checkout_carryover_preserves_author_email_in_session,
    test_switch_carryover_preserves_author_email_in_session,
    test_rebase_rewrite_preserves_author_email_in_session,
    test_rebase_rewrite_preserves_author_email_in_human_record,
    test_status_checkpoint_preserves_author_email_in_session,
);
