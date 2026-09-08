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

mod author_identity;
mod author_identity_rewrites;
mod checkpoint_order;
mod deletions;
mod diff_settings;
mod file_batches;
mod line_attribution;
mod multiple_sessions;
mod partial_staging;
mod prepend_edits;
mod readme_rewrite;
