use super::*;
use repos::test_file::ExpectedLineExt;
use std::time::{Duration, Instant};

#[test]
fn http_fetch_hydrates_captured_tip_after_tracking_ref_moves() {
    http_hydration_uses_operation_revision("fetch", false);
}

#[test]
fn http_pull_hydrates_captured_tip_after_head_and_tracking_ref_move() {
    http_hydration_uses_operation_revision("pull", false);
}

#[test]
fn http_path_pull_hydrates_captured_head_without_tracking_ref_changes() {
    http_hydration_uses_operation_revision("pull", true);
}

fn http_hydration_uses_operation_revision(command: &str, use_path: bool) {
    let server = ReferenceServer::start("127.0.0.1:0").unwrap();
    let url = server.base_url();
    let gates = tempfile::tempdir().unwrap();
    let gate = gates.path().join("fetch");
    let spec = format!("{command}={}", gate.display());
    let local = TestRepo::new_with_daemon_env(&[
        ("GIT_AI_NOTES_BACKEND_KIND", "http"),
        ("GIT_AI_NOTES_BACKEND_URL", &url),
        ("GIT_AI_API_KEY", "transport-revision-test"),
        ("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec),
    ]);
    let source = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    fs::write(source.path().join("file.txt"), "base\n").unwrap();
    source.git_og(&["add", "."]).unwrap();
    source.git_og(&["commit", "-m", "base"]).unwrap();
    source
        .filename("file.txt")
        .assert_committed_lines(crate::lines!["base".unattributed_human()]);
    let base = source
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    local
        .git_og(&["remote", "add", "origin", source.path().to_str().unwrap()])
        .unwrap();
    local.git_og(&["fetch", "origin"]).unwrap();
    local.git_og(&["reset", "--hard", &base]).unwrap();
    let tracking = format!("refs/remotes/origin/{}", source.current_branch());
    local
        .git_og(&["symbolic-ref", "refs/remotes/origin/HEAD", &tracking])
        .unwrap();
    fs::write(source.path().join("file.txt"), "base\nincoming\n").unwrap();
    source.git_og(&["commit", "-am", "incoming"]).unwrap();
    source
        .filename("file.txt")
        .assert_committed_lines(crate::lines![
            "base".unattributed_human(),
            "incoming".unattributed_human()
        ]);
    let incoming = source
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    server
        .store()
        .put(incoming.clone(), "captured incoming note".to_string());
    fs::write(&gate, "").unwrap();
    let completions = local.daemon_total_completion_count();
    let destination = if use_path {
        source.path().to_str().unwrap()
    } else {
        "origin"
    };
    let args = if command == "fetch" {
        vec!["fetch", destination]
    } else {
        vec!["pull", "--ff-only", destination, "HEAD"]
    };
    local.git_without_test_sync_for_test(&args, &[]).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "transport did not reach side-effect gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    local.git_og(&["update-ref", &tracking, &base]).unwrap();
    local.git_og(&["reset", "--hard", &base]).unwrap();
    fs::remove_file(&gate).unwrap();
    local.wait_for_daemon_total_completion_count(completions, completions + 1);
    let db = NotesDatabase::open_at_path(&local.test_home_path().join(".git-ai/internal/notes-db"))
        .unwrap();
    assert_eq!(
        db.get_note(&incoming).unwrap(),
        Some("captured incoming note".to_string()),
        "hydration must use the captured incoming OID, even after its tracking ref changes; {:?}",
        local.daemon_diagnostics()
    );
}
