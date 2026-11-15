//! `notes_backend.*` mutations, asserted on the saved file: the effective kind
//! of an unconfigured backend differs between test builds (git_notes) and
//! production (sqlite), but what `config set`/`unset` persists does not.

use super::*;

fn saved_notes_backend(repo: &TestRepo) -> Option<Value> {
    let config_path = repo.test_home_path().join(".git-ai/config.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(config_path).unwrap()).unwrap();
    saved.get("notes_backend").cloned()
}

fn config(repo: &TestRepo, args: &[&str]) {
    let args = [&["config"], args].concat();
    repo.git_ai(&args)
        .unwrap_or_else(|e| panic!("git-ai {args:?} failed: {e}"));
}

#[test]
fn sqlite_kind_is_settable() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);

    config(&repo, &["set", "notes_backend.kind", "sqlite"]);

    assert_eq!(
        saved_notes_backend(&repo),
        Some(serde_json::json!({"kind": "sqlite"}))
    );
    // An empty patch drops TestRepo's git_notes pin, so this reads the file.
    assert_eq!(
        get_json_with_env(
            &repo,
            "notes_backend.kind",
            &[("GIT_AI_TEST_CONFIG_PATCH", "{}")]
        ),
        "sqlite"
    );
}

#[test]
fn unset_kind_reverts_to_the_default_instead_of_pinning_git_notes() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);

    config(&repo, &["set", "notes_backend.kind", "http"]);
    config(&repo, &["unset", "notes_backend.kind"]);
    assert_eq!(
        saved_notes_backend(&repo),
        None,
        "a backend that is back to defaults is omitted from the file"
    );

    config(&repo, &["set", "notes_backend.kind", "http"]);
    config(
        &repo,
        &["set", "notes_backend.backend_url", "https://example.com"],
    );
    config(&repo, &["unset", "notes_backend.kind"]);
    assert_eq!(
        saved_notes_backend(&repo),
        Some(serde_json::json!({"kind": "sqlite", "backend_url": "https://example.com"}))
    );
}

#[test]
fn unset_backend_url_keeps_an_explicit_kind() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);

    for kind in ["git_notes", "http"] {
        config(&repo, &["set", "notes_backend.kind", kind]);
        config(
            &repo,
            &["set", "notes_backend.backend_url", "https://example.com"],
        );
        config(&repo, &["unset", "notes_backend.backend_url"]);
        assert_eq!(
            saved_notes_backend(&repo),
            Some(serde_json::json!({"kind": kind}))
        );
    }
}
