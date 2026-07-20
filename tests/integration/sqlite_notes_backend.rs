//! End-to-end coverage of the sqlite notes backend (the production default).
//!
//! The shared test suite pins the git-notes backend, so these tests opt back
//! into sqlite explicitly: the daemon gets the backend kind and an isolated
//! notes-db path via env at spawn (the daemon owns note writes), and CLI read
//! invocations pass the same env.

use crate::repos::test_repo::TestRepo;
use git_ai::config::{ConfigPatch, NotesBackendConfig, NotesBackendKind};
use git_ai::model::repository::notes_db::NotesDatabase;
use std::fs;

fn sqlite_backend_repo() -> (TestRepo, tempfile::TempDir, std::path::PathBuf) {
    let temp_root = std::env::temp_dir();
    let temp_root = temp_root.canonicalize().unwrap_or(temp_root);
    let daemon_patch = ConfigPatch {
        allowed_repositories: Some(vec![temp_root.to_string_lossy().replace('\\', "/")]),
        exclude_prompts_in_repositories: Some(vec![]),
        prompt_storage: Some("notes".to_string()),
        notes_backend: Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        }),
        ..Default::default()
    };
    let daemon_patch_json =
        serde_json::to_string(&daemon_patch).expect("serialize daemon config patch");
    let notes_db_dir = tempfile::tempdir().expect("create isolated notes-db directory");
    let notes_db_path = notes_db_dir.path().join("notes-db");
    let notes_db_path_string = notes_db_path.to_string_lossy().to_string();

    let mut repo = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_TEST_CONFIG_PATCH", daemon_patch_json.as_str()),
        ("GIT_AI_TEST_NOTES_DB_PATH", notes_db_path_string.as_str()),
    ]);
    // CLI invocations should use the sqlite backend too.
    repo.patch_git_ai_config(|patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    (repo, notes_db_dir, notes_db_path)
}

fn commit_with_ai_line(repo: &TestRepo) -> String {
    let file_path = repo.path().join("example.txt");
    fs::write(&file_path, "Human line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "example.txt"])
        .unwrap();
    fs::write(&file_path, "Human line\nAI line\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git(&["commit", "-m", "commit with AI line"]).unwrap();
    repo.sync_daemon();
    repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string()
}

#[test]
fn test_commit_writes_note_to_sqlite_not_refs() {
    let (repo, _db_dir, db_path) = sqlite_backend_repo();
    let head = commit_with_ai_line(&repo);

    // The note lives in the sqlite database as a local-primary row...
    let db = NotesDatabase::open_at_path(&db_path).expect("open notes db");
    let note = db.get_note(&head).expect("query notes db");
    let note = note.expect("commit should have a note in the sqlite database");
    assert!(
        note.contains("mock_ai"),
        "note should attribute the AI agent, got: {note}"
    );

    // ...and refs/notes/ai stays empty.
    let refs_list = repo.git(&["notes", "--ref=ai", "list"]).unwrap_or_default();
    assert!(
        refs_list.trim().is_empty(),
        "sqlite backend must not write refs/notes/ai, got: {refs_list}"
    );
}

#[test]
fn test_blame_reads_attribution_from_sqlite() {
    let (repo, _db_dir, db_path) = sqlite_backend_repo();
    let _head = commit_with_ai_line(&repo);

    let db_path_str = db_path.to_string_lossy().to_string();
    let blame = repo
        .git_ai_with_env_without_pre_sync_for_test(
            &["blame", "example.txt"],
            &[("GIT_AI_TEST_NOTES_DB_PATH", db_path_str.as_str())],
        )
        .expect("blame should succeed");
    let ai_line = blame
        .lines()
        .find(|l| l.contains("AI line"))
        .expect("blame should include the AI line");
    assert!(
        ai_line.to_lowercase().contains("mock"),
        "AI line should be attributed to the mock agent, got: {ai_line}"
    );
}

#[test]
fn test_reads_fall_back_to_refs_and_backfill_cache() {
    let (repo, _db_dir, db_path) = sqlite_backend_repo();
    let head = commit_with_ai_line(&repo);

    // Move the note out of the database into refs/notes/ai, simulating a repo
    // with pre-existing git notes (e.g. fetched from a teammate).
    let note = {
        let db = NotesDatabase::open_at_path(&db_path).expect("open notes db");
        db.get_note(&head)
            .expect("query notes db")
            .expect("note present")
    };
    let note_file = repo.path().join("note-content.txt");
    fs::write(&note_file, &note).unwrap();
    repo.git(&[
        "notes",
        "--ref=ai",
        "add",
        "-f",
        "-F",
        note_file.to_str().unwrap(),
        &head,
    ])
    .unwrap();

    // Point the CLI at a fresh, empty database: the read must fall back to
    // refs/notes/ai and still resolve AI attribution.
    let empty_db_dir = tempfile::tempdir().unwrap();
    let empty_db_path = empty_db_dir.path().join("notes-db");
    let empty_db_str = empty_db_path.to_string_lossy().to_string();
    let blame = repo
        .git_ai_with_env_without_pre_sync_for_test(
            &["blame", "example.txt"],
            &[("GIT_AI_TEST_NOTES_DB_PATH", empty_db_str.as_str())],
        )
        .expect("blame should succeed via refs fallback");
    let ai_line = blame
        .lines()
        .find(|l| l.contains("AI line"))
        .expect("blame should include the AI line");
    assert!(
        ai_line.to_lowercase().contains("mock"),
        "fallback read should attribute the AI line, got: {ai_line}"
    );

    // The fallback read backfills the empty database as evictable cache.
    let db = NotesDatabase::open_at_path(&empty_db_path).expect("open backfilled db");
    assert!(
        db.get_note(&head).expect("query").is_some(),
        "refs fallback should backfill the cache"
    );
}

#[test]
fn test_amend_migrates_note_in_sqlite() {
    let (repo, _db_dir, db_path) = sqlite_backend_repo();
    let first = commit_with_ai_line(&repo);

    repo.git(&["commit", "--amend", "-m", "amended message"])
        .unwrap();
    repo.sync_daemon();
    let amended = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    assert_ne!(first, amended);

    let db = NotesDatabase::open_at_path(&db_path).expect("open notes db");
    let note = db.get_note(&amended).expect("query notes db");
    assert!(
        note.is_some(),
        "amended commit should carry the migrated note in sqlite"
    );
}

#[test]
fn test_notes_migrate_between_refs_and_sqlite() {
    let (repo, _db_dir, db_path) = sqlite_backend_repo();
    let head = commit_with_ai_line(&repo);
    let db_path_str = db_path.to_string_lossy().to_string();

    // Export local-primary rows to refs/notes/ai.
    repo.git_ai_with_env_without_pre_sync_for_test(
        &["notes", "migrate", "--to", "git-notes"],
        &[("GIT_AI_TEST_NOTES_DB_PATH", db_path_str.as_str())],
    )
    .expect("export to git-notes should succeed");
    let refs_list = repo.git(&["notes", "--ref=ai", "list"]).unwrap();
    assert!(
        !refs_list.trim().is_empty(),
        "export should populate refs/notes/ai"
    );

    // Import refs/notes/ai into a fresh database as local-primary rows.
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh_path = fresh_dir.path().join("notes-db");
    let fresh_str = fresh_path.to_string_lossy().to_string();
    repo.git_ai_with_env_without_pre_sync_for_test(
        &["notes", "migrate", "--to", "sqlite"],
        &[("GIT_AI_TEST_NOTES_DB_PATH", fresh_str.as_str())],
    )
    .expect("import to sqlite should succeed");
    let db = NotesDatabase::open_at_path(&fresh_path).expect("open imported db");
    assert!(
        db.get_note(&head).expect("query").is_some(),
        "import should copy the note into the fresh database"
    );
}
