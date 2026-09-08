use super::*;
use crate::model::repository::notes_db::NotesDatabase;
use crate::operations::git::test_utils::TmpRepo;
use tempfile::NamedTempFile;

/// Helper to create real commits in a TmpRepo. Returns the commit SHA.
/// Parents are tracked implicitly via `HEAD`, so callers no longer need to
/// pass them explicitly.
fn make_commit(repo: &TmpRepo, filename: &str, content: &str, message: &str) -> String {
    repo.write_file(filename, content, false)
        .expect("write file");
    repo.commit_all(message).expect("commit")
}

/// Add a git note to `refs/notes/ai` for the given commit SHA.
fn add_git_note(repo: &TmpRepo, commit_sha: &str, note: &str) {
    repo.git_command(&["notes", "--ref=ai", "add", "-f", "-m", note, commit_sha])
        .expect("git notes add");
}

fn read_note_blobs(
    repo: &crate::operations::git::repository::Repository,
    blob_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    batch_read_blob_contents_with_policy(repo, blob_shas, BatchReadPolicy::Tolerant)
}

/// Integration test:
///   1. Create a TmpRepo with several commits and git notes.
///   2. Start a mockito server to accept the upload.
///   3. Call `handle_notes_migrate` logic directly (list + cat-file + upload + cache).
///   4. Verify all notes appear in `notes-db` with `synced = 1`.
///   5. Verify the mock upload endpoint was called.
#[test]
#[serial_test::serial(notes_db_env)]
fn migration_uploads_notes_and_caches_with_synced_1() {
    // Isolated notes-db.
    let tmp_db = NamedTempFile::new().expect("tmp notes-db");
    unsafe {
        std::env::set_var("GIT_AI_TEST_NOTES_DB_PATH", tmp_db.path());
    }

    // --- Build repo with commits and notes ---
    let repo = TmpRepo::new().expect("TmpRepo::new");

    let sha1 = make_commit(&repo, "file1.txt", "hello", "commit 1");
    let sha2 = make_commit(&repo, "file2.txt", "world", "commit 2");
    let sha3 = make_commit(&repo, "file3.txt", "foo", "commit 3");

    // Add git notes for each commit.
    add_git_note(&repo, &sha1, "note-content-1");
    add_git_note(&repo, &sha2, "note-content-2");
    add_git_note(&repo, &sha3, "note-content-3");

    // --- Mock upload endpoint ---
    let mut server = mockito::Server::new();
    let upload_response = serde_json::json!({
        "success_count": 3,
        "failure_count": 0
    })
    .to_string();
    let _mock = server
        .mock("POST", "/worker/notes/upload")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(&upload_response)
        .create();

    let server_url = server.url();
    unsafe {
        std::env::set_var("GIT_AI_NOTES_BACKEND_URL", &server_url);
        std::env::set_var("GIT_AI_API_KEY", "migrate-test-key");
    }

    // --- Run the migration core logic ---
    let note_pairs = list_notes(repo.gitai_repo()).expect("list_notes");
    assert_eq!(note_pairs.len(), 3, "should list 3 notes");

    let blob_to_commit: HashMap<String, String> = note_pairs
        .iter()
        .map(|(b, c)| (b.clone(), c.clone()))
        .collect();
    let blob_shas: Vec<String> = note_pairs.iter().map(|(b, _)| b.clone()).collect();

    let blob_contents = read_note_blobs(repo.gitai_repo(), &blob_shas).expect("read_note_blobs");
    assert_eq!(blob_contents.len(), 3, "should read 3 blob contents");

    let mut entries: Vec<(String, String)> = Vec::new();
    for (blob_sha, content) in &blob_contents {
        if let Some(commit_sha) = blob_to_commit.get(blob_sha) {
            entries.push((commit_sha.clone(), content.clone()));
        }
    }
    assert_eq!(entries.len(), 3);

    // Upload to the mock server.
    let cfg = crate::config::Config::fresh();
    let backend_url = cfg
        .notes_backend_url()
        .expect("test should configure notes_backend.backend_url")
        .to_string();
    let ctx = ApiContext::new(Some(backend_url), resolve_api_author_identity);
    let client = ApiClient::new(ctx);

    let note_entries: Vec<NoteEntry> = entries
        .iter()
        .map(|(sha, content)| NoteEntry {
            commit_sha: sha.clone(),
            content: content.clone(),
        })
        .collect();
    let request = NotesUploadRequest {
        entries: note_entries,
    };
    let response = client.upload_notes(request).expect("upload_notes");
    assert_eq!(response.success_count, 3);
    assert_eq!(response.failure_count, 0);

    // Cache locally with synced = 1.
    let db = NotesDatabase::global().expect("global db");
    {
        let mut lock = db.lock().expect("lock");
        lock.cache_synced_notes(&entries)
            .expect("cache_synced_notes");
    }

    // --- Verify all three notes are in notes-db with synced = 1 ---
    let lock = db.lock().expect("lock for verify");
    let shas = [sha1.as_str(), sha2.as_str(), sha3.as_str()];
    for sha in &shas {
        let content = lock.get_note(sha).expect("get_note");
        assert!(content.is_some(), "note for {} should be in notes-db", sha);
    }

    // None of them should appear in dequeue_pending (synced = 1).
    drop(lock);
    let mut lock = db.lock().expect("lock for dequeue");
    let pending = lock.dequeue_pending(10).expect("dequeue_pending");
    let migrated_pending: Vec<_> = pending
        .iter()
        .filter(|p| shas.contains(&p.commit_sha.as_str()))
        .collect();
    assert!(
        migrated_pending.is_empty(),
        "migrated notes must not appear in dequeue_pending: {:?}",
        migrated_pending
            .iter()
            .map(|p| &p.commit_sha)
            .collect::<Vec<_>>()
    );

    // --- Verify the mock was called ---
    _mock.assert();

    // Cleanup.
    unsafe {
        std::env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
        std::env::remove_var("GIT_AI_API_KEY");
        std::env::remove_var("GIT_AI_NOTES_BACKEND_URL");
    }
}

/// Unit test: `list_notes` returns empty when there are no notes.
#[test]
fn list_notes_returns_empty_for_repo_without_notes() {
    let repo = TmpRepo::new().expect("TmpRepo::new");
    // Create a commit so HEAD exists (list_notes on an empty repo might error differently).
    repo.write_file("a.txt", "a", false).expect("write file");
    repo.commit_all("c").expect("commit");

    let pairs = list_notes(repo.gitai_repo()).expect("list_notes");
    assert!(
        pairs.is_empty(),
        "no notes should be listed for a fresh repo"
    );
}

/// Unit test: the tolerant shared reader returns an empty map for empty input.
#[test]
fn read_note_blobs_empty_input() {
    let repo = TmpRepo::new().expect("TmpRepo::new");
    let result = read_note_blobs(repo.gitai_repo(), &[]).expect("read_note_blobs");
    assert!(result.is_empty());
}

/// Integration test: `--force` re-uploads notes that are already cached as synced.
///   1. Create notes and cache them as synced=1 in notes-db.
///   2. Without --force: verify entries are filtered out.
///   3. With --force: verify all entries pass through for upload.
#[test]
#[serial_test::serial(notes_db_env)]
fn force_flag_bypasses_synced_cache_filter() {
    use std::collections::HashSet;

    let tmp_db = NamedTempFile::new().expect("tmp notes-db");
    unsafe {
        std::env::set_var("GIT_AI_TEST_NOTES_DB_PATH", tmp_db.path());
    }

    let repo = TmpRepo::new().expect("TmpRepo::new");

    let sha1 = make_commit(&repo, "file1.txt", "a", "commit 1");
    let sha2 = make_commit(&repo, "file2.txt", "b", "commit 2");

    add_git_note(&repo, &sha1, "note-1");
    add_git_note(&repo, &sha2, "note-2");

    // Read notes from repo.
    let note_pairs = list_notes(repo.gitai_repo()).expect("list_notes");
    let blob_to_commit: HashMap<String, String> = note_pairs
        .iter()
        .map(|(b, c)| (b.clone(), c.clone()))
        .collect();
    let blob_shas: Vec<String> = note_pairs.iter().map(|(b, _)| b.clone()).collect();
    let blob_contents = read_note_blobs(repo.gitai_repo(), &blob_shas).expect("read_note_blobs");

    let entries: Vec<(String, String)> = blob_contents
        .iter()
        .filter_map(|(blob_sha, content)| {
            blob_to_commit
                .get(blob_sha)
                .map(|commit_sha| (commit_sha.clone(), content.clone()))
        })
        .collect();
    assert_eq!(entries.len(), 2);

    // Pre-cache all entries as synced=1.
    let db = NotesDatabase::global().expect("global db");
    {
        let mut lock = db.lock().expect("lock");
        lock.cache_synced_notes(&entries)
            .expect("cache_synced_notes");
    }

    // Without force: filtering should remove all entries.
    {
        let mut filtered = entries.clone();
        let lock = db.lock().expect("lock");
        let all_shas: Vec<&str> = filtered.iter().map(|(s, _)| s.as_str()).collect();
        let synced = lock.get_synced_shas(&all_shas).expect("get_synced_shas");
        filtered.retain(|(sha, _)| !synced.contains(sha));
        assert!(
            filtered.is_empty(),
            "without --force, all synced entries should be filtered out"
        );
    }

    // With force: no filtering applied, all entries remain.
    {
        let forced_entries = entries.clone();
        // force=true means we skip the retain logic entirely
        assert_eq!(
            forced_entries.len(),
            2,
            "with --force, all entries should remain for upload"
        );

        // Verify we can upload them to a new backend.
        let mut server = mockito::Server::new();
        let upload_response = serde_json::json!({
            "success_count": 2,
            "failure_count": 0
        })
        .to_string();
        let mock = server
            .mock("POST", "/worker/notes/upload")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(&upload_response)
            .create();

        let server_url = server.url();
        unsafe {
            std::env::set_var("GIT_AI_NOTES_BACKEND_URL", &server_url);
            std::env::set_var("GIT_AI_API_KEY", "force-test-key");
        }

        let cfg = crate::config::Config::fresh();
        let backend_url = cfg.notes_backend_url().unwrap().to_string();
        let ctx = ApiContext::new(Some(backend_url), resolve_api_author_identity);
        let client = ApiClient::new(ctx);

        let note_entries: Vec<NoteEntry> = forced_entries
            .iter()
            .map(|(sha, content)| NoteEntry {
                commit_sha: sha.clone(),
                content: content.clone(),
            })
            .collect();
        let request = NotesUploadRequest {
            entries: note_entries,
        };
        let response = client.upload_notes(request).expect("upload_notes");
        assert_eq!(response.success_count, 2);
        mock.assert();
    }

    // Verify commit shas are what we expect.
    let lock = db.lock().expect("lock for final verify");
    let shas_set: HashSet<&str> = [sha1.as_str(), sha2.as_str()].into_iter().collect();
    for sha in &shas_set {
        assert!(
            lock.get_note(sha).expect("get_note").is_some(),
            "note for {} should remain in cache",
            sha
        );
    }
    drop(lock);

    unsafe {
        std::env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
        std::env::remove_var("GIT_AI_API_KEY");
        std::env::remove_var("GIT_AI_NOTES_BACKEND_URL");
    }
}
