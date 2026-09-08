use super::{AuthorConfig, ExpectedLineExt, TestRepo, fs};

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

crate::reuse_tests_in_worktree!(
    test_known_human_record_includes_email,
    test_session_record_human_author_includes_email,
);
