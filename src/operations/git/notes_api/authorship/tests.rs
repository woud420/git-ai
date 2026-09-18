use super::*;
use crate::model::repository::notes_db::NotesDatabase;
use crate::operations::git::notes_store::db_read_note;
use crate::operations::git::test_utils::TmpRepo;

fn isolated_notes_test(name: &str, test: impl FnOnce()) {
    let name = format!("operations::git::notes_api::authorship::tests::{name}");
    if std::env::var("GIT_AI_BATCH_NOTES_TEST").as_deref() == Ok(name.as_str()) {
        test();
        return;
    }
    // The notes DB is a OnceLock, so changing its path between tests cannot
    // isolate it. A fresh test process gives each case its own database.
    let directory = tempfile::tempdir().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", &name, "--nocapture", "--test-threads=1"])
        .env("GIT_AI_BATCH_NOTES_TEST", &name)
        .env(
            "GIT_AI_TEST_NOTES_DB_PATH",
            directory.path().join("notes.db"),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

#[test]
fn cached_and_refs_notes_preserve_backend_parsing_and_backfill_rules() {
    isolated_notes_test(
        "cached_and_refs_notes_preserve_backend_parsing_and_backfill_rules",
        || {
            let repo = TmpRepo::new().unwrap();
            let shas: Vec<String> = (1..=4).map(|n| format!("{n:040x}")).collect();
            let mut log = AuthorshipLog::new();
            log.metadata.base_commit_sha = "original-base".to_owned();
            let valid = log.serialize_to_string().unwrap();
            log.metadata.schema_version = "authorship/999.0.0".to_owned();
            let future = log.serialize_to_string().unwrap();
            let refs = vec![
                (shas[0].clone(), valid.clone()),
                (shas[1].clone(), future.clone()),
                (shas[2].clone(), valid.clone()),
                (shas[3].clone(), valid.clone()),
            ];
            crate::operations::git::refs::notes_add_batch(repo.gitai_repo(), &refs).unwrap();
            let db = NotesDatabase::global().unwrap();
            {
                let mut db = db.lock().unwrap();
                db.upsert_local_note(&shas[2], &future).unwrap();
                db.upsert_local_note(&shas[3], "invalid cache entry")
                    .unwrap();
            }
            let http =
                read_batch_for_backend(repo.gitai_repo(), &shas, NotesBackendKind::Http).unwrap();
            assert_eq!(http.len(), 2);
            assert_eq!(http[&shas[0]].metadata.base_commit_sha, shas[0]);
            assert_eq!(http[&shas[2]].metadata.base_commit_sha, "original-base");
            assert_eq!(http[&shas[2]].metadata.schema_version, "authorship/999.0.0");
            assert!(
                db_read_note(&shas[0]).is_none(),
                "HTTP refs fallback does not backfill"
            );
            assert!(db_read_note(&shas[1]).is_none());
            assert!(!http.contains_key(&shas[3]), "invalid cache hits mask refs");

            let sqlite =
                read_batch_for_backend(repo.gitai_repo(), &shas, NotesBackendKind::Sqlite).unwrap();
            assert_eq!(sqlite.len(), 3);
            for sha in &shas[..3] {
                assert_eq!(sqlite[sha].metadata.base_commit_sha, "original-base");
            }
            assert_eq!(
                sqlite[&shas[1]].metadata.schema_version,
                "authorship/999.0.0"
            );
            assert_eq!(db_read_note(&shas[0]), Some(valid));
            assert_eq!(db_read_note(&shas[1]), Some(future));
            assert!(!sqlite.contains_key(&shas[3]));
        },
    );
}

#[test]
fn large_cached_blame_batches_preserve_notes_beyond_sqlite_bind_limit() {
    isolated_notes_test(
        "large_cached_blame_batches_preserve_notes_beyond_sqlite_bind_limit",
        || {
            let repo = TmpRepo::new().unwrap();
            let mut shas: Vec<String> = (1..=40_000).map(|n| format!("{n:040x}")).collect();
            let log = AuthorshipLog::new().serialize_to_string().unwrap();
            let first = shas[0].clone();
            let last = shas.last().unwrap().clone();
            let db = NotesDatabase::global().unwrap();
            {
                let mut db = db.lock().unwrap();
                db.upsert_local_note(&first, &log).unwrap();
                db.upsert_local_note(&last, &log).unwrap();
            }
            shas.extend(std::iter::repeat_n(first.clone(), 40_000));
            for backend in [NotesBackendKind::Sqlite, NotesBackendKind::Http] {
                let batch = read_batch_for_backend(repo.gitai_repo(), &shas, backend).unwrap();
                assert_eq!(
                    batch.len(),
                    2,
                    "large batch lost cached notes for {backend:?}"
                );
                assert!(batch.contains_key(&first));
                assert!(batch.contains_key(&last));
            }
        },
    );
}
