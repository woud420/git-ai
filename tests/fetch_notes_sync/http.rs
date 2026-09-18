use super::*;

#[test]
fn fetch_http_warms_captured_incoming_revision() {
    assert_http_fetch_cache_warming(false);
}

#[test]
fn unchanged_fetch_http_does_not_infer_incoming_history_from_live_remote_head() {
    assert_http_fetch_cache_warming(true);
}

fn assert_http_fetch_cache_warming(already_fetched: bool) {
    let (source, oid, note) = attributed_source();
    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let backend_url = server.base_url();
    server.store().put(oid.clone(), note.clone());
    let local = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", &backend_url),
        ("GIT_AI_API_KEY", "fetch-sync-test-key"),
    ]);
    add_remote(&local, "upstream", &source);
    if already_fetched {
        local.git_og(&["fetch", "upstream"]).unwrap();
        local
            .git_og(&["remote", "set-head", "upstream", "--auto"])
            .unwrap();
    }
    let db_path = local.test_home_path().join(".git-ai/internal/notes-db");
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap(),
        None
    );

    run_fetch(&local, &["fetch", "upstream"]).unwrap();
    assert_eq!(
        NotesDatabase::open_at_path(&db_path)
            .unwrap()
            .get_note(&oid)
            .unwrap(),
        (!already_fetched).then_some(note)
    );
    assert!(local.read_authorship_note(&oid).is_none());
}
