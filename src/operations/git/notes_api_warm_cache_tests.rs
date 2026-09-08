use super::*;

// --- warm_cache_for_remote tests ---
//
// These tests verify the core behavior of `warm_cache_for_remote`:
//
// 1. It fetches notes from the HTTP backend and stores them with `synced = 1`.
// 2. It skips SHAs already present in notes-db (not included in the request).
//
// Design notes on the `NOTES_DB` `OnceLock` singleton:
//
// `NotesDatabase::global()` uses a `OnceLock` that initialises the DB path
// *once per process*.  Both tests set `GIT_AI_TEST_NOTES_DB_PATH` to a fresh
// temp file before their first DB call.  The first test to run initialises the
// OnceLock; subsequent tests in the same process reuse the same DB file path
// regardless of what `GIT_AI_TEST_NOTES_DB_PATH` says.
//
// Strategy: both tests use `NotesDatabase::global()` for all reads and writes
// (pre-population and post-call verification) rather than direct file-level
// connections.  Because the tests run serially (`#[serial]`) and each uses
// distinct commit SHAs, shared DB state doesn't cause false-negative assertions.
//
// Test 1 sets `GIT_AI_TEST_NOTES_DB_PATH` which initialises the OnceLock if
// it hasn't been set yet.  Test 2 also sets it but will use whatever path was
// already locked.  Both tests clear DB state relevant to their own SHAs via
// `get_note` assertions on distinct SHAs, so they don't interfere.

/// Unit test: `warm_cache_for_remote` fetches notes from a mock HTTP server
/// and stores them in `notes-db` with `synced = 1`.
///
/// Steps:
/// 1. Point `NOTES_DB` at a fresh temp file (via `GIT_AI_TEST_NOTES_DB_PATH`).
/// 2. Spin up a mockito server returning two notes.
/// 3. Create a `TmpRepo` with two commits.
/// 4. Call `warm_cache_for_remote`.
/// 5. Verify both SHAs appear in `notes-db` with `synced = 1` via `NotesDatabase::global()`.
#[test]
#[serial_test::serial(notes_db_env)]
fn warm_cache_for_remote_populates_db_with_synced_1() {
    use crate::model::repository::notes_db::NotesDatabase;
    use crate::operations::git::test_utils::TmpRepo;
    use tempfile::NamedTempFile;

    // Set the test DB path before the first DB call so the OnceLock picks it up.
    let tmp_db = NamedTempFile::new().expect("tmp notes-db");
    unsafe {
        std::env::set_var("GIT_AI_TEST_NOTES_DB_PATH", tmp_db.path());
    }

    // Build a TmpRepo with two commits.
    let repo = TmpRepo::new().expect("TmpRepo::new");

    repo.write_file("warm1.txt", "warm1", false)
        .expect("write file");
    let sha1 = repo.commit_all("warm-commit-1").expect("commit 1");

    repo.write_file("warm2.txt", "warm2", false)
        .expect("write file");
    let sha2 = repo.commit_all("warm-commit-2").expect("commit 2");

    // Spin up a mockito server that returns notes for both SHAs.
    let mut server = mockito::Server::new();
    let notes_json = serde_json::json!({
        "notes": {
            sha1.clone(): "note-content-1",
            sha2.clone(): "note-content-2"
        }
    })
    .to_string();

    let _mock = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/worker/notes/".to_string()),
        )
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(&notes_json)
        .create();

    let server_url = server.url();
    unsafe {
        std::env::set_var("GIT_AI_NOTES_BACKEND_URL", &server_url);
        // Provide a fake API key so `has_api_key()` returns true and the
        // auth guard in `warm_cache_for_remote` does not short-circuit.
        std::env::set_var("GIT_AI_API_KEY", "warm-cache-test-key");
    }

    // Execute.
    let result = warm_cache_for_remote(repo.gitai_repo(), "origin");
    assert!(result.is_ok(), "warm_cache_for_remote failed: {:?}", result);

    // Verify via NotesDatabase::global() (the same DB the function wrote to).
    let db = NotesDatabase::global().expect("global db");
    let lock = db.lock().expect("lock");

    let content1 = lock.get_note(&sha1).expect("get sha1");
    let content2 = lock.get_note(&sha2).expect("get sha2");

    assert_eq!(
        content1,
        Some("note-content-1".to_string()),
        "sha1 should be cached with correct content"
    );
    assert_eq!(
        content2,
        Some("note-content-2".to_string()),
        "sha2 should be cached with correct content"
    );

    // Rows must NOT appear in dequeue_pending (cache_synced_notes inserts synced = 1).
    drop(lock);
    let mut lock = db.lock().expect("lock for dequeue check");
    let pending = lock.dequeue_pending(10).expect("dequeue");
    let warm_pending: Vec<_> = pending
        .iter()
        .filter(|p| p.commit_sha == sha1 || p.commit_sha == sha2)
        .collect();
    assert!(
        warm_pending.is_empty(),
        "cache_synced rows must not appear in dequeue_pending: {:?}",
        warm_pending
            .iter()
            .map(|p| &p.commit_sha)
            .collect::<Vec<_>>()
    );

    // Cleanup.
    unsafe {
        std::env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
        std::env::remove_var("GIT_AI_API_KEY");
        std::env::remove_var("GIT_AI_NOTES_BACKEND_URL");
    }
}

/// Unit test: `warm_cache_for_remote` skips SHAs already present in `notes-db`.
///
/// Steps:
/// 1. Pre-populate `notes-db` with sha1 via `cache_synced_notes`.
/// 2. Spin up a mockito server returning sha2 only.
///    The mock matches only requests whose query contains sha2 —
///    if sha1 were incorrectly included it would still match, but we verify
///    indirectly that sha1's content was not overwritten.
/// 3. Call `warm_cache_for_remote` with a TmpRepo containing both commits.
/// 4. Verify sha1's content is unchanged ("already-cached-note").
/// 5. Verify sha2 was fetched and cached with `synced = 1`.
#[test]
#[serial_test::serial(notes_db_env)]
fn warm_cache_for_remote_skips_already_cached_shas() {
    use crate::model::repository::notes_db::NotesDatabase;
    use crate::operations::git::test_utils::TmpRepo;
    use tempfile::NamedTempFile;

    // Set the test DB path (may be ignored if OnceLock was already set by
    // `warm_cache_for_remote_populates_db_with_synced_1` in the same process,
    // but we still set it for freshness when running this test in isolation).
    let tmp_db = NamedTempFile::new().expect("tmp notes-db");
    unsafe {
        std::env::set_var("GIT_AI_TEST_NOTES_DB_PATH", tmp_db.path());
    }

    // Build TmpRepo with two commits.
    let repo = TmpRepo::new().expect("TmpRepo::new");

    repo.write_file("skip1.txt", "s1", false)
        .expect("write file");
    let sha1 = repo.commit_all("skip-c1").expect("commit 1");

    repo.write_file("skip2.txt", "s2", false)
        .expect("write file");
    let sha2 = repo.commit_all("skip-c2").expect("commit 2");

    // Pre-populate notes-db with sha1 via the global singleton.
    {
        let db = NotesDatabase::global().expect("global db");
        let mut lock = db.lock().expect("lock");
        lock.cache_synced_notes(&[(sha1.clone(), "already-cached-note".to_string())])
            .expect("cache_synced_notes sha1");
    }

    // Mock server: use two mocks to verify sha1 is NOT in the request.
    //
    // - Mock A: matches requests where the query contains sha2 but NOT sha1.
    //   Since mockito doesn't have a `Not` matcher, we approximate this by
    //   requiring the query equals exactly sha2 (no comma-separated prefix/suffix).
    //   `commits=<sha2>` means only sha2 was requested.
    // - Mock B: fallback that matches everything else → returns 500 so sha2
    //   is NOT cached if sha1 was erroneously included.
    //
    // If warm_cache correctly filters sha1, mock A matches and sha2 is cached.
    // If warm_cache incorrectly sends sha1 too, the query is `sha1,sha2` or
    // `sha2,sha1`, which won't match the exact-sha2 regex → mock B fires → 500
    // error → sha2 is NOT cached → `assert_eq!(content2, ...)` fails.
    let sha2_note_json = serde_json::json!({
        "notes": { sha2.clone(): "note-content-skip-2" }
    })
    .to_string();

    // Exact query: commits=<sha2> only.
    let exact_sha2_query = format!("commits={}", sha2);

    let mut server = mockito::Server::new();
    // Mock A: exact query with only sha2.
    let _mock_ok = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/worker/notes/".to_string()),
        )
        .match_query(mockito::Matcher::Exact(exact_sha2_query))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(&sha2_note_json)
        .create();
    // Mock B: fallback → 500.
    let _mock_fallback = server
        .mock(
            "GET",
            mockito::Matcher::Regex(r"^/worker/notes/".to_string()),
        )
        .with_status(500)
        .with_body(r#"{"error":"unexpected request with sha1 in query"}"#)
        .create();

    let server_url = server.url();
    unsafe {
        std::env::set_var("GIT_AI_NOTES_BACKEND_URL", &server_url);
        std::env::set_var("GIT_AI_API_KEY", "skip-test-key");
    }

    let result = warm_cache_for_remote(repo.gitai_repo(), "origin");
    assert!(result.is_ok(), "warm_cache_for_remote failed: {:?}", result);

    // Verify via the global DB.
    let db = NotesDatabase::global().expect("global db");
    let lock = db.lock().expect("lock");

    // sha1 must retain its pre-cached content unchanged.
    let content1 = lock.get_note(&sha1).expect("get sha1");
    assert_eq!(
        content1,
        Some("already-cached-note".to_string()),
        "sha1 content must not change — warm_cache must not overwrite cached entries"
    );

    // sha2 must now be cached with the server-returned content.
    let content2 = lock.get_note(&sha2).expect("get sha2");
    assert_eq!(
        content2,
        Some("note-content-skip-2".to_string()),
        "sha2 should have been fetched and cached"
    );

    // The mock must have been called (warm_cache made at least one HTTP request).
    _mock_ok.assert();

    // Cleanup.
    unsafe {
        std::env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
        std::env::remove_var("GIT_AI_API_KEY");
        std::env::remove_var("GIT_AI_NOTES_BACKEND_URL");
    }
}
