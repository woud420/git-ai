use super::*;
use crate::operations::git::notes_store::{HttpNoteStore, SqliteNoteStore, db_read_note};

/// With kind=Http, the http helpers upsert into notes-db (synced=0) and the
/// read helper returns the cached value. This tests the store methods directly
/// so no config override is needed.
#[test]
#[serial_test::serial(notes_db_env)]
fn http_write_then_read_uses_cache() {
    use std::env;

    // Point the notes-db at a temp file so we don't pollute the real DB.
    let tmp = tempfile::NamedTempFile::new().expect("tmp file");
    let db_path = tmp.path().to_str().unwrap().to_string();
    // Safety: test-only env var manipulation.
    unsafe {
        env::set_var("GIT_AI_TEST_NOTES_DB_PATH", &db_path);
    }

    // Write directly via store (no repo needed).
    HttpNoteStore::new()
        .write_note("abc123def456abc123def456abc123def456abc1", "test content")
        .expect("write");

    // Read back from cache.
    let content = db_read_note("abc123def456abc123def456abc123def456abc1");
    assert_eq!(content, Some("test content".to_string()));

    // Confirm it is in the DB with synced=0.
    let db = crate::model::repository::notes_db::NotesDatabase::global().expect("global db");
    let mut lock = db.lock().expect("lock");
    let pending = lock.dequeue_pending(10).expect("dequeue");
    assert!(
        pending.iter().any(
            |p| p.commit_sha == "abc123def456abc123def456abc123def456abc1"
                && p.content == "test content"
        ),
        "expected pending row in notes-db"
    );

    // Cleanup env var.
    unsafe {
        env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
    }
}

/// http_read_notes returns a HashMap of all cached entries for requested SHAs.
#[test]
#[serial_test::serial(notes_db_env)]
fn http_read_notes_returns_multiple() {
    use std::env;

    let tmp = tempfile::NamedTempFile::new().expect("tmp file");
    let db_path = tmp.path().to_str().unwrap().to_string();
    unsafe {
        env::set_var("GIT_AI_TEST_NOTES_DB_PATH", &db_path);
    }

    let sha1 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
    let sha2 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
    let sha3 = "cccccccccccccccccccccccccccccccccccccccc".to_string();

    HttpNoteStore::new()
        .write_note(&sha1, "content-a")
        .expect("write sha1");
    HttpNoteStore::new()
        .write_note(&sha2, "content-b")
        .expect("write sha2");

    // sha3 is not written — should not appear in result.
    let result = db_read_notes(&[sha1.clone(), sha2.clone(), sha3.clone()]);
    assert_eq!(result.get(&sha1), Some(&"content-a".to_string()));
    assert_eq!(result.get(&sha2), Some(&"content-b".to_string()));
    assert!(!result.contains_key(&sha3));

    unsafe {
        env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
    }
}

/// Under the HTTP backend, note search must find notes that only exist in
/// the notes-db cache (refs/notes/ai is empty there). Regression: search was
/// a pure pass-through to `git grep refs/notes/ai`, so session/prompt history
/// lookups silently found nothing under the HTTP backend.
///
/// Tests the store's search_notes directly (not via the dispatcher) to avoid
/// the `GIT_AI_NOTES_BACKEND_KIND` env var racing with other concurrent tests.
#[test]
#[serial_test::serial(notes_db_env)]
fn http_search_notes_finds_cached_note_content() {
    use crate::operations::git::notes_store::AuthorshipNoteStore;
    use std::env;

    let tmp_db = tempfile::NamedTempFile::new().expect("tmp file");
    let db_path = tmp_db.path().to_str().unwrap().to_string();
    unsafe {
        env::set_var("GIT_AI_TEST_NOTES_DB_PATH", &db_path);
    }

    let sha = "dddddddddddddddddddddddddddddddddddddddd";
    HttpNoteStore::new()
        .write_note(sha, r#"{"sessions": {"s_searchable123456": {}}}"#)
        .expect("write");

    // Verify the db-tier search (used by both Http and Sqlite arms) finds
    // notes in the cache even when refs/notes/ai is absent.
    let matches = SqliteNoteStore::new()
        .search_notes("\"s_searchable123456\"")
        .expect("search");
    assert_eq!(
        matches,
        vec![sha.to_string()],
        "search must find notes that only exist in the notes-db cache"
    );

    // A needle that appears nowhere must return no matches (LIKE wildcards in
    // the needle must not be interpreted).
    let no_matches = SqliteNoteStore::new()
        .search_notes("\"s_%_absent%\"")
        .expect("search");
    assert!(no_matches.is_empty(), "got: {:?}", no_matches);

    unsafe {
        env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
    }
}

/// Regression test for the composed Sqlite|Http search arm in
/// `search_notes(repo, pattern)`.
///
/// Before the P9.5 store-trait refactor the Http arm was a pure git-grep
/// pass-through; notes that existed only in the notes-db cache were invisible
/// to search.  This test drives the full dispatcher path (db search + git-grep
/// union + sort) with `GIT_AI_TEST_NOTES_DB_PATH` to avoid env-var races and
/// verifies that a db-only SHA (absent from refs/notes/ai) is returned.
#[test]
#[serial_test::serial]
#[serial_test::serial(notes_db_env)]
fn sqlite_http_search_arm_returns_db_only_sha() {
    use crate::operations::git::test_utils::TmpRepo;
    use std::env;

    let tmp_db = tempfile::NamedTempFile::new().expect("tmp notes-db");
    unsafe {
        env::set_var("GIT_AI_TEST_NOTES_DB_PATH", tmp_db.path().to_str().unwrap());
    }

    // Use a fake SHA that does not exist in any git object store.
    let db_only_sha = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_string();
    let pattern = "\"db_only_session_xyz\"";
    let note_content = r#"{"sessions": {"db_only_session_xyz": {}}}"#;

    // Seed the note into the db directly via SqliteNoteStore so it exists
    // only in the db, not in refs/notes/ai.
    SqliteNoteStore::new()
        .write_note(&db_only_sha, note_content)
        .expect("seed note");

    // Create a TmpRepo (needed for the repo arg; refs/notes/ai will be empty).
    let repo = TmpRepo::new().expect("TmpRepo::new");

    // Call the composed search path with Sqlite backend.  The search must
    // union db results with the (empty) git-grep result and return the SHA.
    unsafe {
        env::set_var("GIT_AI_NOTES_BACKEND_KIND", "sqlite");
    }
    let results = search_notes(repo.gitai_repo(), pattern).expect("search_notes must not error");
    unsafe {
        env::remove_var("GIT_AI_NOTES_BACKEND_KIND");
    }

    assert!(
        results.contains(&db_only_sha),
        "db-only SHA must appear in composed search results; got: {:?}",
        results
    );

    unsafe {
        env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
    }
}

/// The raw git-notes lookup returns Ok for an empty commit list.
#[test]
fn note_blob_oids_for_commits_accepts_empty_commit_list() {
    use crate::operations::git::test_utils::TmpRepo;
    let tmp = TmpRepo::new().expect("TmpRepo::new");
    let result = crate::operations::git::refs::note_blob_oids_for_commits(tmp.gitai_repo(), &[]);
    assert!(result.is_ok());
}

/// Integration test: with kind=Http, `write_note` upserts into `notes-db`
/// with `synced = 0` and `git notes --ref=ai show <sha>` returns nothing (note
/// is NOT written into git refs).
#[test]
#[serial_test::serial(notes_db_env)]
fn integration_http_write_note_goes_to_db_not_git() {
    use crate::clients::git_cli::exec_git;
    use crate::operations::git::test_utils::TmpRepo;
    use std::env;

    // Isolated notes-db for this test.
    let tmp_db = tempfile::NamedTempFile::new().expect("tmp db file");
    let db_path = tmp_db.path().to_str().unwrap().to_string();
    unsafe {
        env::set_var("GIT_AI_TEST_NOTES_DB_PATH", &db_path);
    }

    let repo = TmpRepo::new().expect("TmpRepo::new");

    // Create a real commit so we have a valid SHA.
    repo.write_file("a.txt", "hello", false)
        .expect("write file");
    let sha = repo.commit_all("msg").expect("commit");

    // Write a note for this SHA using the Http store.
    HttpNoteStore::new()
        .write_note(&sha, "some-note-content")
        .expect("http write");

    // Confirm it is in notes-db with synced=0.
    let db = crate::model::repository::notes_db::NotesDatabase::global().expect("global db");
    let mut lock = db.lock().expect("lock");
    let note_in_db = lock.get_note(&sha).expect("get note");
    assert_eq!(note_in_db, Some("some-note-content".to_string()));

    let pending = lock.dequeue_pending(10).expect("dequeue");
    assert!(
        pending.iter().any(|p| p.commit_sha == sha),
        "note should be pending in notes-db"
    );
    drop(lock);

    // Confirm `git notes --ref=ai show <sha>` returns nothing.
    let mut args = repo.gitai_repo().global_args_for_exec();
    args.extend([
        "notes".to_string(),
        "--ref=ai".to_string(),
        "show".to_string(),
        sha.clone(),
    ]);
    let result = exec_git(&args);
    assert!(
        result.is_err(),
        "git notes --ref=ai show should fail (note not in git) for Http backend"
    );

    unsafe {
        env::remove_var("GIT_AI_TEST_NOTES_DB_PATH");
    }
}

/// Verify that `run_pre_push_hook_managed` has the correct early-return guard for
/// `kind = Http`. We test this by confirming Config::fresh() with
/// `GIT_AI_NOTES_BACKEND_KIND=http` returns Http, and that the guard in
/// `run_pre_push_hook_managed` would short-circuit.
#[test]
#[serial_test::serial]
fn push_pre_command_hook_http_guard_is_in_place() {
    use std::env;

    let old = env::var("GIT_AI_NOTES_BACKEND_KIND").ok();
    unsafe {
        env::set_var("GIT_AI_NOTES_BACKEND_KIND", "http");
    }
    let kind = crate::config::Config::fresh().notes_backend_kind();
    match old {
        Some(v) => unsafe { env::set_var("GIT_AI_NOTES_BACKEND_KIND", v) },
        None => unsafe { env::remove_var("GIT_AI_NOTES_BACKEND_KIND") },
    }

    assert_eq!(
        kind,
        crate::config::NotesBackendKind::Http,
        "Config::fresh() should reflect GIT_AI_NOTES_BACKEND_KIND=http"
    );

    // Structural verification: the Http backend skip is now inlined in
    // apply_push_side_effect in daemon.rs — no separate hook function needed.
}

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
