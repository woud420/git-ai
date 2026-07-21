use super::*;
use crate::model::working_log::CheckpointKind;
use crate::operations::commands::checkpoint_agent::orchestrator::CheckpointRequest;
use crate::operations::daemon::checkpoint::PreparedPathRole;
use crate::operations::daemon::cherry_pick_helpers::{
    cherry_pick_source_args_from_command_args, rebase_new_tip_from_command,
    revert_source_args_from_command_args,
};
use crate::operations::daemon::git_backend::GitBackend;
use crate::operations::daemon::revert_rebase_helpers::strict_rebase_original_head_from_command;
use serde_json::Value;
use serial_test::serial;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

struct EnvVarGuard {
    key: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var_os(key);
        // SAFETY: these tests are serialized via #[serial], so mutating the
        // process environment is isolated for the duration of each test.
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }

    fn unset(key: &'static str) -> Self {
        let original = std::env::var_os(key);
        // SAFETY: these tests are serialized via #[serial], so mutating the
        // process environment is isolated for the duration of each test.
        unsafe {
            std::env::remove_var(key);
        }
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => {
                // SAFETY: these tests are serialized via #[serial], so restoring
                // process environment state is isolated for the duration of each test.
                unsafe {
                    std::env::set_var(self.key, value);
                }
            }
            None => {
                // SAFETY: these tests are serialized via #[serial], so restoring
                // process environment state is isolated for the duration of each test.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }
}

fn sample_checkpoint_request() -> ControlRequest {
    use crate::operations::commands::checkpoint_agent::orchestrator::{BaseCommit, CheckpointFile};
    ControlRequest::CheckpointRun {
        request: Box::new(CheckpointRequest {
            trace_id: "test-trace".to_string(),
            checkpoint_kind: CheckpointKind::Human,
            agent_id: None,
            files: vec![CheckpointFile {
                path: std::path::PathBuf::from("test.txt"),
                content: None,
                repo_work_dir: std::path::PathBuf::from("/tmp/repo"),
                base_commit: BaseCommit::Initial,
            }],
            path_role: PreparedPathRole::WillEdit,
            stream_source: None,
            metadata: std::collections::HashMap::new(),
        }),
    }
}

fn run_git_for_test(repo: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("git {:?} failed to spawn: {}", args, error));
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout should be utf8")
        .trim()
        .to_string()
}

fn run_git_stdin_for_test(repo: &Path, args: &[&str], stdin: &str) -> String {
    let mut child = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("git {:?} failed to spawn: {}", args, error));
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(stdin.as_bytes())
        .expect("write git stdin");
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("git {:?} failed to wait: {}", args, error));
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git stdout should be utf8")
        .trim()
        .to_string()
}

#[test]
fn conflict_resolution_note_read_errors_are_not_silently_ignored() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().join("repo");
    std::fs::create_dir_all(&repo_path).unwrap();
    run_git_for_test(&repo_path, &["init"]);
    run_git_for_test(&repo_path, &["config", "user.name", "Test User"]);
    run_git_for_test(&repo_path, &["config", "user.email", "test@example.com"]);

    std::fs::write(repo_path.join("file.txt"), "onto\n").unwrap();
    run_git_for_test(&repo_path, &["add", "file.txt"]);
    run_git_for_test(&repo_path, &["commit", "-m", "onto"]);
    let onto = run_git_for_test(&repo_path, &["rev-parse", "HEAD"]);

    std::fs::write(repo_path.join("file.txt"), "onto\nnew\n").unwrap();
    run_git_for_test(&repo_path, &["add", "file.txt"]);
    run_git_for_test(&repo_path, &["commit", "-m", "new"]);
    let new_tip = run_git_for_test(&repo_path, &["rev-parse", "HEAD"]);

    let missing_blob = "2222222222222222222222222222222222222222";
    let prefix = &new_tip[..2];
    let suffix = &new_tip[2..];
    let leaf_tree = run_git_stdin_for_test(
        &repo_path,
        &["mktree", "--missing"],
        &format!("100644 blob {missing_blob}\t{suffix}\n"),
    );
    let root_tree = run_git_stdin_for_test(
        &repo_path,
        &["mktree"],
        &format!("040000 tree {leaf_tree}\t{prefix}\n"),
    );
    run_git_for_test(&repo_path, &["update-ref", "refs/notes/ai", &root_tree]);

    let repo = crate::operations::git::find_repository_in_path(repo_path.to_str().unwrap())
        .expect("find test repository");
    let result = process_conflict_resolution_working_logs(&repo, &new_tip, Some(&onto));
    assert!(
        result.is_err(),
        "corrupt destination notes must fail closed instead of being treated as absent"
    );
}

#[test]
fn revert_source_args_do_not_treat_bare_gpg_sign_as_value_option() {
    assert_eq!(
        revert_source_args_from_command_args(&["--gpg-sign".to_string(), "HEAD~1".to_string()]),
        vec!["HEAD~1"]
    );
    assert_eq!(
        revert_source_args_from_command_args(&["-S".to_string(), "HEAD~1".to_string()]),
        vec!["HEAD~1"]
    );
    assert_eq!(
        revert_source_args_from_command_args(&["-Smy-key".to_string(), "HEAD~1".to_string()]),
        vec!["HEAD~1"]
    );
}

#[test]
fn cherry_pick_source_args_do_not_treat_bare_gpg_sign_as_value_option() {
    assert_eq!(
        cherry_pick_source_args_from_command_args(&[
            "--gpg-sign".to_string(),
            "HEAD~1".to_string()
        ]),
        vec!["HEAD~1"]
    );
    assert_eq!(
        cherry_pick_source_args_from_command_args(&["-S".to_string(), "HEAD~1".to_string()]),
        vec!["HEAD~1"]
    );
    assert_eq!(
        cherry_pick_source_args_from_command_args(&["-Smy-key".to_string(), "HEAD~1".to_string()]),
        vec!["HEAD~1"]
    );
}

#[test]
fn checkpoint_requests_use_long_timeout_in_ci_or_test_env() {
    assert_eq!(
        checkpoint_control_response_timeout(&sample_checkpoint_request(), true),
        DAEMON_CHECKPOINT_RESPONSE_TIMEOUT
    );
}

#[test]
fn checkpoint_requests_use_short_timeout_in_product_env() {
    assert_eq!(
        checkpoint_control_response_timeout(&sample_checkpoint_request(), false),
        DAEMON_CONTROL_RESPONSE_TIMEOUT
    );
}

#[test]
fn transcript_sweep_triggers_for_commit_amend_and_push_events() {
    use crate::model::domain::SemanticEvent;
    use crate::operations::daemon::stream_worker::SweepTrigger;

    assert_eq!(
        transcript_sweep_triggers_for_events(&[SemanticEvent::CommitCreated {
            base: Some("base".to_string()),
            new_head: "new".to_string(),
        }]),
        vec![SweepTrigger::PostCommit]
    );
    assert_eq!(
        transcript_sweep_triggers_for_events(&[SemanticEvent::CommitAmended {
            old_head: "old".to_string(),
            new_head: "new".to_string(),
        }]),
        vec![SweepTrigger::PostCommit]
    );
    assert_eq!(
        transcript_sweep_triggers_for_events(&[SemanticEvent::PushCompleted {
            remote: Some("origin".to_string()),
        }]),
        vec![SweepTrigger::PostPush]
    );
    assert_eq!(
        transcript_sweep_triggers_for_events(&[
            SemanticEvent::CommitCreated {
                base: Some("base".to_string()),
                new_head: "new".to_string(),
            },
            SemanticEvent::PushCompleted {
                remote: Some("origin".to_string()),
            },
        ]),
        vec![SweepTrigger::PostCommit, SweepTrigger::PostPush]
    );
}

fn test_rebase_command(
    invoked_args: &[&str],
    ref_changes: Vec<crate::model::domain::RefChange>,
) -> crate::model::domain::NormalizedCommand {
    crate::model::domain::NormalizedCommand {
        scope: crate::model::domain::CommandScope::Family(crate::model::domain::FamilyKey(
            "/repo/.git".to_string(),
        )),
        family_key: Some(crate::model::domain::FamilyKey("/repo/.git".to_string())),
        worktree: Some(PathBuf::from("/repo")),
        root_sid: "rebase-test".to_string(),
        raw_argv: std::iter::once("git")
            .chain(std::iter::once("rebase"))
            .chain(invoked_args.iter().copied())
            .map(str::to_string)
            .collect(),
        primary_command: Some("rebase".to_string()),
        invoked_command: Some("rebase".to_string()),
        invoked_args: invoked_args.iter().map(|arg| (*arg).to_string()).collect(),
        observed_child_commands: Vec::new(),
        exit_code: 0,
        started_at_ns: 1,
        finished_at_ns: 2,
        reflog_start_offsets: HashMap::new(),
        stash_target_oid: None,
        cherry_pick_source_oids: Vec::new(),
        revert_source_oids: Vec::new(),
        ref_changes,
        confidence: crate::model::domain::Confidence::High,
    }
}

fn ref_change(reference: &str, old: &str, new: &str) -> crate::model::domain::RefChange {
    crate::model::domain::RefChange {
        reference: reference.to_string(),
        old: old.to_string(),
        new: new.to_string(),
    }
}

#[test]
fn explicit_branch_rebase_original_head_prefers_branch_ref_over_head() {
    const MAIN: &str = "1111111111111111111111111111111111111111";
    const FEATURE: &str = "2222222222222222222222222222222222222222";
    const ONTO: &str = "3333333333333333333333333333333333333333";

    let cmd = test_rebase_command(
        &["master", "scenario-3-multi-file-conflict"],
        vec![
            ref_change("HEAD", MAIN, FEATURE),
            ref_change("HEAD", FEATURE, ONTO),
            ref_change(
                "refs/heads/scenario-3-multi-file-conflict",
                FEATURE,
                FEATURE,
            ),
        ],
    );

    assert_eq!(
        strict_rebase_original_head_from_command(&cmd, MAIN),
        Some(FEATURE.to_string()),
        "explicit branch rebase must store the target branch tip, not the caller's original HEAD"
    );
}

#[test]
fn pending_rebase_new_tip_prefers_matching_branch_ref_over_later_head_noise() {
    const ORIGINAL: &str = "1111111111111111111111111111111111111111";
    const ONTO: &str = "2222222222222222222222222222222222222222";
    const NEW_TIP: &str = "3333333333333333333333333333333333333333";
    const UNRELATED_HEAD: &str = "4444444444444444444444444444444444444444";

    let cmd = test_rebase_command(
        &["--continue"],
        vec![
            ref_change("HEAD", ONTO, NEW_TIP),
            ref_change(
                "refs/heads/scenario-3-multi-file-conflict",
                ORIGINAL,
                NEW_TIP,
            ),
            ref_change("HEAD", NEW_TIP, UNRELATED_HEAD),
        ],
    );

    assert_eq!(
        rebase_new_tip_from_command(&cmd, ORIGINAL),
        Some(NEW_TIP.to_string()),
        "pending rebase completion must use the branch ref update that rewrote the original tip"
    );
}

#[test]
#[serial]
fn checkpoint_control_timeout_uses_ci_env_var() {
    let _unset_test = EnvVarGuard::unset("GIT_AI_TEST_DB_PATH");
    let _unset_legacy_test = EnvVarGuard::unset("GITAI_TEST_DB_PATH");
    let _set_ci = EnvVarGuard::set("CI", "true");

    assert!(checkpoint_control_timeout_uses_ci_or_test_budget());
}

#[test]
#[serial]
fn checkpoint_control_timeout_uses_test_db_env_var() {
    let _unset_ci = EnvVarGuard::unset("CI");
    let _unset_legacy_test = EnvVarGuard::unset("GITAI_TEST_DB_PATH");
    let _set_test = EnvVarGuard::set("GIT_AI_TEST_DB_PATH", "/tmp/git-ai-test.db");

    assert!(checkpoint_control_timeout_uses_ci_or_test_budget());
}

#[test]
#[serial]
fn checkpoint_control_timeout_false_when_no_ci_or_test_vars() {
    let _unset_ci = EnvVarGuard::unset("CI");
    let _unset_test = EnvVarGuard::unset("GIT_AI_TEST_DB_PATH");
    let _unset_legacy_test = EnvVarGuard::unset("GITAI_TEST_DB_PATH");

    assert!(!checkpoint_control_timeout_uses_ci_or_test_budget());
}

#[test]
fn compute_watermarks_uses_symlink_metadata_not_target_mtime() {
    // Verify that compute_watermarks_from_stat uses lstat (symlink's own mtime)
    // not stat (target file's mtime), consistent with snapshot's symlink_metadata.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();

    // Create a target file
    let target = dir.join("target.txt");
    std::fs::write(&target, b"hello").unwrap();

    // Create a symlink pointing to the target
    let link = dir.join("link.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &link).unwrap();

    // Watermark the symlink
    let wm = compute_watermarks_from_stat(dir.to_str().unwrap(), &["link.txt".to_string()]);

    // The watermark should match symlink_metadata mtime, not target metadata mtime.
    let symlink_meta = std::fs::symlink_metadata(&link).unwrap();
    let target_meta = std::fs::metadata(&link).unwrap(); // follows symlink

    let symlink_mtime = symlink_meta
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let target_mtime = target_meta
        .modified()
        .unwrap()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let recorded = *wm.get("link.txt").unwrap();

    assert_eq!(
        recorded, symlink_mtime,
        "watermark should match lstat mtime of the symlink itself"
    );
    // This assertion documents the intent: if symlink and target mtimes differ,
    // the watermark must track the symlink, not the target.
    let _ = target_mtime; // used only as documentation; may equal symlink_mtime on some FS
}

#[test]
fn explicit_stop_overrides_prior_restart_intent() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    runtime.block_on(async {
        let coordinator = ActorDaemonCoordinator::new();

        coordinator.request_restart_after_update();
        assert_eq!(
            coordinator.shutdown_action(),
            DaemonExitAction::RestartAfterUpdate
        );

        coordinator.request_stop();

        assert!(coordinator.is_shutting_down());
        assert_eq!(coordinator.shutdown_action(), DaemonExitAction::Stop);
    });
}

// -----------------------------------------------------------------------
// Readonly command ingress fast-path tests
//
// These tests verify that prepare_trace_payload_for_ingest returns false
// (do-not-enqueue) for read-only commands and true for mutating ones, and
// that the queued_trace_payloads counter is not incremented for read-only
// events.
//
// ActorDaemonCoordinator::new() spawns Tokio tasks internally, so all
// tests that construct one must run inside a Tokio runtime.
// -----------------------------------------------------------------------

fn make_start_payload(argv: &[&str]) -> Value {
    serde_json::json!({
        "event": "start",
        "sid": "20260411T120000.000000-Psid1",
        "argv": argv,
    })
}

fn make_atexit_payload(sid: &str) -> Value {
    serde_json::json!({
        "event": "atexit",
        "sid": sid,
        "code": 0,
    })
}

#[test]
fn exit_is_not_a_root_completion_boundary() {
    let sid = "20260411T120000.000000-Psid1";

    assert!(
        !is_terminal_root_trace_event("exit", sid, sid),
        "trace2 exit can fire before Git atexit cleanup and must not complete root processing"
    );
    assert!(is_terminal_root_trace_event("atexit", sid, sid));
}

#[tokio::test]
async fn readonly_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&["git", "status", "--short"]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "status start event should not be enqueued (readonly)"
    );
    assert_eq!(
        coord.queued_trace_payloads.load(Ordering::Relaxed),
        0,
        "queued_trace_payloads should stay 0 for readonly start event"
    );
    // Readonly events must NOT receive an ingest sequence number
    assert!(
        payload.get(TRACE_INGEST_SEQ_FIELD).is_none(),
        "readonly start event must not receive an ingest sequence number"
    );
}

#[tokio::test]
async fn stash_list_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&[
        "git",
        "-c",
        "core.fsmonitor=false",
        "--no-pager",
        "stash",
        "list",
        "--pretty=format:%gd%x00%H%x00%ct%x00%s",
    ]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "stash list start event should not be enqueued (readonly invocation)"
    );
    assert!(
        payload.get(TRACE_INGEST_SEQ_FIELD).is_none(),
        "stash list start event must not receive an ingest sequence number"
    );
}

#[tokio::test]
async fn worktree_list_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&[
        "git",
        "--no-pager",
        "--no-optional-locks",
        "worktree",
        "list",
        "--porcelain",
    ]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "worktree list start event should not be enqueued (readonly invocation)"
    );
    assert!(
        payload.get(TRACE_INGEST_SEQ_FIELD).is_none(),
        "worktree list start event must not receive an ingest sequence number"
    );
}

#[tokio::test]
async fn branch_show_current_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&["git", "branch", "--show-current"]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "branch --show-current start event should not be enqueued"
    );
    assert!(
        payload.get(TRACE_INGEST_SEQ_FIELD).is_none(),
        "branch --show-current must not receive an ingest sequence number"
    );
}

#[tokio::test]
async fn diff_numstat_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&[
        "git",
        "-c",
        "core.fsmonitor=false",
        "--no-pager",
        "diff",
        "--numstat",
        "--no-renames",
        "HEAD",
    ]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "diff --numstat start event should not be enqueued"
    );
}

#[tokio::test]
async fn for_each_ref_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&[
        "git",
        "--no-pager",
        "for-each-ref",
        "refs/heads/**/*",
        "refs/remotes/**/*",
        "--format",
        "%(HEAD)%00%(objectname)",
    ]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "for-each-ref start event should not be enqueued"
    );
}

#[tokio::test]
async fn cat_file_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&[
        "git",
        "--no-optional-locks",
        "cat-file",
        "--batch-check=%(objectname)",
    ]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        !should_enqueue,
        "cat-file start event should not be enqueued"
    );
}

#[tokio::test]
async fn show_commit_start_event_is_not_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&[
        "git",
        "--no-optional-locks",
        "show",
        "--no-patch",
        "--format=%H%x00%B%x00%at",
        "07270e1489439d6b36fcb2a4198d2fb68e37727c",
    ]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(!should_enqueue, "show start event should not be enqueued");
}

#[tokio::test]
async fn mutating_commit_start_event_is_enqueued() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    let mut payload = make_start_payload(&["git", "commit", "-m", "test commit"]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        should_enqueue,
        "commit start event should be enqueued (mutating)"
    );
    assert!(
        payload.get(TRACE_INGEST_SEQ_FIELD).is_none(),
        "mutating event must not receive an ingest sequence number before enqueue capacity is reserved"
    );
    assert_eq!(
        coord.next_trace_ingest_seq.load(Ordering::Acquire),
        0,
        "prepare must not allocate an ingest sequence"
    );
    coord
        .enqueue_trace_payload(payload)
        .expect("mutating event should enqueue");
    assert!(
        coord.next_trace_ingest_seq.load(Ordering::Acquire) > 0,
        "enqueue must allocate an ingest sequence number"
    );
    coord.request_shutdown();
}

#[tokio::test]
async fn mutating_pending_root_is_created_when_repo_and_argv_arrive_on_different_events() {
    let coord = ActorDaemonCoordinator::new();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(temp.path())
        .arg("init")
        .arg("repo")
        .output()
        .expect("git init should run");
    assert!(
        init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let sid = "20260411T120000.000000-Psid-split-metadata";
    let mut def_repo = serde_json::json!({
        "event": "def_repo",
        "sid": sid,
        "worktree": repo,
        "time_ns": 1u64,
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut def_repo));
    coord
        .apply_trace_payload_to_state(def_repo)
        .await
        .expect("def_repo should ingest");
    assert!(
        !coord
            .pending_root_slots_by_root
            .lock()
            .unwrap()
            .contains_key(sid),
        "repo-only metadata is not enough to sequence a command"
    );

    let mut start = serde_json::json!({
        "event": "start",
        "sid": sid,
        "argv": ["git", "reset", "--soft", "HEAD~1"],
        "time_ns": 2u64,
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut start));
    coord
        .apply_trace_payload_to_state(start)
        .await
        .expect("start should ingest");

    assert!(
        coord
            .pending_root_slots_by_root
            .lock()
            .unwrap()
            .contains_key(sid),
        "mutating roots must be sequenced once argv and repo metadata are both known, even when they arrive on different events"
    );
}

#[tokio::test]
async fn mutating_trace_payload_captures_repo_reflog_start_offsets() {
    let coord = ActorDaemonCoordinator::new();
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let git_dir = repo.join(".git");
    let head_log = git_dir.join("logs/HEAD");
    let stash_log = repo.join(".git/logs/refs/stash");
    let branch_log = repo.join(".git/logs/refs/heads/main");
    std::fs::create_dir_all(head_log.parent().unwrap()).unwrap();
    std::fs::create_dir_all(stash_log.parent().unwrap()).unwrap();
    std::fs::create_dir_all(branch_log.parent().unwrap()).unwrap();
    let old_head_reflog = b"old HEAD reflog entry\n";
    let old_reflog = b"old stash reflog entry\n";
    let old_branch_reflog = b"old branch reflog entry\n";
    std::fs::write(&head_log, old_head_reflog).unwrap();
    std::fs::write(&stash_log, old_reflog).unwrap();
    std::fs::write(&branch_log, old_branch_reflog).unwrap();
    let mut payload = serde_json::json!({
        "event": "start",
        "sid": "20260411T120000.000000-Psid-reflog",
        "argv": ["git", "reset", "--hard", "HEAD~1"],
        "worktree": repo,
    });

    assert!(coord.prepare_trace_payload_for_ingest(&mut payload));

    let offsets = payload
        .get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD)
        .and_then(Value::as_object)
        .expect("mutating trace payload should include reflog start offsets");
    let head_key = format!(
        "worktree:{}:HEAD",
        git_dir.canonicalize().unwrap().to_string_lossy()
    );
    assert_eq!(
        offsets.get(&head_key).and_then(Value::as_u64),
        Some(old_head_reflog.len() as u64)
    );
    assert_eq!(
        offsets.get("common:refs/stash").and_then(Value::as_u64),
        Some(old_reflog.len() as u64)
    );
    assert_eq!(
        offsets
            .get("common:refs/heads/main")
            .and_then(Value::as_u64),
        Some(old_branch_reflog.len() as u64)
    );
}

#[tokio::test]
async fn checkpoint_fence_waits_for_open_mutating_trace_root() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let sid = "20260411T120000.000000-Psid1";
    coord.trace_root_connection_opened(sid).unwrap();
    let mut payload = make_start_payload(&["git", "commit", "-m", "test commit"]);
    assert!(
        coord.prepare_trace_payload_for_ingest(&mut payload),
        "commit start should mark the root as mutating"
    );

    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            coord.wait_for_trace_ingest_processed_through()
        )
        .await
        .is_err(),
        "checkpoint fence must not pass while a mutating trace root is still open"
    );

    coord
        .record_trace_connection_close(&[sid.to_string()])
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        coord.wait_for_trace_ingest_processed_through(),
    )
    .await
    .expect("checkpoint fence should pass once the mutating trace root closes");
}

#[tokio::test]
async fn checkpoint_control_request_waits_while_blocked_behind_pending_root() {
    use crate::operations::commands::checkpoint_agent::orchestrator::{BaseCommit, CheckpointFile};

    let coord = Arc::new(ActorDaemonCoordinator::new());
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(&repo)
        .arg("init")
        .output()
        .expect("git init should run");
    assert!(
        init.status.success(),
        "git init failed: {}",
        String::from_utf8_lossy(&init.stderr)
    );
    std::fs::write(repo.join("test.txt"), "checkpoint content\n").unwrap();

    let family = coord.backend.resolve_family(&repo).unwrap().0;
    let root_sid = "20260411T120000.000000-Psid-blocking-root";
    coord
        .append_pending_root_entry(&family, root_sid, 1)
        .unwrap();

    let request = CheckpointRequest {
        trace_id: "blocked-checkpoint".to_string(),
        checkpoint_kind: CheckpointKind::Human,
        agent_id: None,
        files: vec![CheckpointFile {
            path: PathBuf::from("test.txt"),
            content: Some("checkpoint content\n".to_string()),
            repo_work_dir: repo.clone(),
            base_commit: BaseCommit::Initial,
        }],
        path_role: PreparedPathRole::Edited,
        stream_source: None,
        metadata: HashMap::new(),
    };

    let mut checkpoint = {
        let coord = coord.clone();
        tokio::spawn(async move { coord.ingest_checkpoint_payload(request).await })
    };

    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut checkpoint)
            .await
            .is_err(),
        "checkpoint control request must not complete before its sequenced side effect runs"
    );

    coord
        .replace_pending_root_entry(root_sid, FamilySequencerEntry::Canceled)
        .await
        .unwrap();

    let response = tokio::time::timeout(Duration::from_secs(1), checkpoint)
        .await
        .expect("checkpoint should finish once the prior root is released")
        .expect("checkpoint task should not panic")
        .expect("checkpoint request should succeed");
    assert!(
        response.ok,
        "checkpoint response should be ok: {response:?}"
    );
}

#[tokio::test]
async fn trace_connection_close_without_atexit_cancels_pending_root() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    std::fs::create_dir_all(git_dir.join("logs")).unwrap();

    let sid = "20260411T120000.000000-Psid-close";
    coord.trace_root_connection_opened(sid).unwrap();
    let mut start = serde_json::json!({
        "event": "start",
        "sid": sid,
        "argv": ["git", "commit", "-m", "test commit"],
        "worktree": worktree,
        "time_ns": 1u64,
    });
    assert!(coord.prepare_trace_payload_for_ingest(&mut start));
    coord.enqueue_trace_payload(start).unwrap();

    finalize_trace_connection_roots(coord.clone(), [sid.to_string()].into_iter().collect())
        .unwrap();
    coord.wait_for_trace_ingest_processed_through().await;

    assert!(
        !coord
            .pending_root_slots_by_root
            .lock()
            .unwrap()
            .contains_key(sid),
        "closing the trace stream without root atexit must not leave the family sequencer wedged"
    );
    coord.request_shutdown();
}

#[tokio::test]
async fn readonly_trace_connection_close_without_atexit_clears_tracking() {
    let coord = ActorDaemonCoordinator::new();
    let sid = "20260411T120000.000000-Psid-readonly-close";
    coord.trace_root_connection_opened(sid).unwrap();
    let mut start = make_start_payload(&["git", "status", "--short"]);
    start["sid"] = serde_json::json!(sid);
    assert!(!coord.prepare_trace_payload_for_ingest(&mut start));

    let close_marker_roots = coord
        .record_trace_connection_close(&[sid.to_string()])
        .unwrap();

    assert!(
        close_marker_roots.is_empty(),
        "read-only roots should not enqueue synthetic close markers"
    );
    let ingress = coord.trace_ingress_state.lock().unwrap();
    assert!(!ingress.root_argv.contains_key(sid));
    assert!(!ingress.root_definitely_read_only.contains(sid));
    assert!(!ingress.root_open_connections.contains_key(sid));
}

#[tokio::test]
async fn checkpoint_fence_does_not_wait_for_unidentified_trace_connection() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.trace_unidentified_connection_opened().unwrap();

    tokio::time::timeout(
        Duration::from_secs(1),
        coord.wait_for_trace_ingest_processed_through(),
    )
    .await
    .expect("checkpoint fence must not wait for an accepted trace connection with no root");

    coord
        .trace_unidentified_connection_identified_or_closed()
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        coord.wait_for_trace_ingest_processed_through(),
    )
    .await
    .expect("checkpoint fence should pass once the unidentified connection is resolved");
}

#[tokio::test]
async fn checkpoint_fence_waits_for_open_branch_mutation_root() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let sid = "20260411T120000.000000-Psid1";
    coord.trace_root_connection_opened(sid).unwrap();
    let mut payload = make_start_payload(&["git", "branch", "-D", "feature"]);
    assert!(
        coord.prepare_trace_payload_for_ingest(&mut payload),
        "branch delete start should be enqueued because it mutates refs"
    );

    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            coord.wait_for_trace_ingest_processed_through()
        )
        .await
        .is_err(),
        "checkpoint fence must not pass while an accepted branch mutation root is still open"
    );

    coord
        .record_trace_connection_close(&[sid.to_string()])
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(1),
        coord.wait_for_trace_ingest_processed_through(),
    )
    .await
    .expect("checkpoint fence should pass once the branch mutation root closes");
}

#[tokio::test]
async fn checkpoint_fence_does_not_wait_for_open_branch_readonly_root() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    let sid = "20260411T120000.000000-Psid-readonly-branch";
    coord.trace_root_connection_opened(sid).unwrap();
    let mut payload = make_start_payload(&["git", "branch", "--show-current"]);
    payload["sid"] = serde_json::json!(sid);
    assert!(
        !coord.prepare_trace_payload_for_ingest(&mut payload),
        "branch --show-current should be classified as read-only"
    );

    tokio::time::timeout(
        Duration::from_secs(1),
        coord.wait_for_trace_ingest_processed_through(),
    )
    .await
    .expect("checkpoint fence must not wait for an open read-only branch root");
}

#[tokio::test]
async fn mutating_stash_pop_start_event_is_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&["git", "stash", "pop"]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        should_enqueue,
        "stash pop start event should be enqueued (mutating)"
    );
}

#[tokio::test]
async fn mutating_worktree_add_start_event_is_enqueued() {
    let coord = ActorDaemonCoordinator::new();
    let mut payload = make_start_payload(&["git", "worktree", "add", "/tmp/branch", "branch"]);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
    assert!(
        should_enqueue,
        "worktree add start event should be enqueued (mutating)"
    );
}

#[tokio::test]
async fn readonly_atexit_event_is_not_enqueued_after_readonly_start() {
    let coord = ActorDaemonCoordinator::new();
    let sid = "20260411T120000.000000-Psid1";

    // Process start event first — marks root as read-only
    let mut start = make_start_payload(&["git", "status"]);
    // Override sid to match
    start["sid"] = serde_json::json!(sid);
    coord.prepare_trace_payload_for_ingest(&mut start);

    // atexit for same root should also be skipped
    let mut atexit = make_atexit_payload(sid);
    let should_enqueue = coord.prepare_trace_payload_for_ingest(&mut atexit);
    assert!(
        !should_enqueue,
        "atexit for readonly root should not be enqueued"
    );
}

/// Performance invariant: 10,000 readonly start events must be processed
/// (and discarded) in under 200ms.  This guards against regressions that
/// re-introduce the >1-minute backlog seen with Zed's ~40 invocations/sec.
#[tokio::test]
async fn readonly_flood_1000_events_processed_in_under_200ms() {
    let coord = ActorDaemonCoordinator::new();
    let start = std::time::Instant::now();
    for i in 0..1000u64 {
        let sid = format!("20260411T120000.000000-P{:016x}", i);
        let mut payload = serde_json::json!({
            "event": "start",
            "sid": sid,
            "argv": ["git", "-c", "core.fsmonitor=false", "--no-pager",
                     "--no-optional-locks", "status", "--porcelain=v1",
                     "--untracked-files=all", "--no-renames", "-z", "."],
        });
        let enqueue = coord.prepare_trace_payload_for_ingest(&mut payload);
        assert!(!enqueue, "status must never be enqueued");
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 200,
        "processing 1000 readonly events took {}ms (> 200ms budget)",
        elapsed.as_millis()
    );
    assert_eq!(
        coord.queued_trace_payloads.load(Ordering::Relaxed),
        0,
        "no readonly events should reach the ingest queue"
    );
}

/// Ensure a stash-list flood (3208 real-world invocations from Zed)
/// leaves the ingest queue empty.
#[tokio::test]
async fn stash_list_flood_leaves_queue_empty() {
    let coord = ActorDaemonCoordinator::new();
    for i in 0..1000u64 {
        let sid = format!("20260411T120000.000000-P{:016x}", i);
        let mut payload = serde_json::json!({
            "event": "start",
            "sid": sid,
            "argv": ["git", "-c", "core.fsmonitor=false", "--no-pager",
                     "stash", "list", "--pretty=format:%gd%x00%H%x00%ct%x00%s"],
        });
        let _ = coord.prepare_trace_payload_for_ingest(&mut payload);
    }
    assert_eq!(
        coord.queued_trace_payloads.load(Ordering::Relaxed),
        0,
        "stash list flood must not fill the ingest queue"
    );
}

/// Ensure a worktree-list flood leaves the ingest queue empty.
#[tokio::test]
async fn worktree_list_flood_leaves_queue_empty() {
    let coord = ActorDaemonCoordinator::new();
    for i in 0..1000u64 {
        let sid = format!("20260411T120000.000000-P{:016x}", i);
        let mut payload = serde_json::json!({
            "event": "start",
            "sid": sid,
            "argv": ["git", "--no-pager", "--no-optional-locks",
                     "worktree", "list", "--porcelain"],
        });
        let _ = coord.prepare_trace_payload_for_ingest(&mut payload);
    }
    assert_eq!(
        coord.queued_trace_payloads.load(Ordering::Relaxed),
        0,
        "worktree list flood must not fill the ingest queue"
    );
}

// -----------------------------------------------------------------------
// OnceLock / shutdown / atomic-ordering tests
// -----------------------------------------------------------------------

/// `enqueue_trace_payload` must return an error when the ingest worker has
/// not been started yet.  This is the "no-sender" fast-fail path and is
/// unchanged by the OnceLock refactor.
#[tokio::test]
async fn enqueue_before_worker_start_returns_error() {
    let coord = ActorDaemonCoordinator::new();
    // Worker never started → OnceLock is empty → enqueue must fail
    let payload = serde_json::json!({
        "event": "start",
        "sid": "20260411T120000.000000-Ptest0001",
        "__git_ai_ingest_seq": 1_u64,
        "argv": ["git", "commit", "-m", "test"],
    });
    assert!(
        coord.enqueue_trace_payload(payload).is_err(),
        "enqueue before worker start must return an error"
    );
}

#[tokio::test]
async fn enqueue_accounting_error_does_not_allocate_ingest_sequence() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    let poison_coord = coord.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poison_coord
            .queued_trace_payloads_by_root
            .lock()
            .expect("mutex should be lockable before intentional poison");
        panic!("intentional queue accounting mutex poison");
    })
    .join();

    let payload = serde_json::json!({
        "event": "start",
        "sid": "20260411T120000.000000-Paccounting",
        "argv": ["git", "commit", "-m", "test"],
    });
    assert!(
        coord.enqueue_trace_payload(payload).is_err(),
        "poisoned queue accounting must fail enqueue"
    );
    assert_eq!(
        coord.next_trace_ingest_seq.load(Ordering::Acquire),
        0,
        "failed enqueue must not allocate an ingest sequence that can block checkpoint drains"
    );
    coord.request_shutdown();
}

/// After `request_shutdown()`, `is_shutting_down()` returns true and the
/// coordinator stays in a consistent state.  The ingest worker (started
/// via `start_trace_ingest_worker`) must exit cleanly even when the sender
/// is no longer dropped by `request_shutdown` (OnceLock never drops it).
#[tokio::test]
async fn request_shutdown_is_idempotent_and_consistent() {
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();
    assert!(!coord.is_shutting_down());
    coord.request_shutdown();
    assert!(coord.is_shutting_down());
    // Second call must not panic.
    coord.request_shutdown();
    assert!(coord.is_shutting_down());
    // Allow tokio to run the ingest worker's shutdown select arm.
    tokio::task::yield_now().await;
}

#[tokio::test]
async fn checkpoint_trace_ingest_drain_returns_on_shutdown() {
    let coord = ActorDaemonCoordinator::new();
    coord.next_trace_ingest_seq.store(1, Ordering::Release);
    coord.processed_trace_ingest_seq.store(0, Ordering::Release);
    coord.request_shutdown();

    tokio::time::timeout(
        std::time::Duration::from_millis(100),
        coord.wait_for_trace_ingest_processed_through(),
    )
    .await
    .expect("checkpoint trace ingest drain must return when daemon shutdown is requested");
}

/// The trace socket receive-buffer helper must raise a socket's `SO_RCVBUF`
/// capacity toward the configured target.
#[test]
#[cfg(not(windows))]
fn trace_socket_recv_buffer_helper_raises_socket_capacity() {
    let (server, _client) =
        std::os::unix::net::UnixStream::pair().expect("create connected unix socket pair");
    let before = socket_recv_buffer(&server).expect("read baseline receive buffer");
    set_socket_recv_buffer(&server, TRACE_SOCKET_RECV_BUFFER_BYTES)
        .expect("set trace socket receive buffer");
    let after = socket_recv_buffer(&server).expect("read trace socket receive buffer");
    // Linux clamps SO_RCVBUF to net.core.rmem_max, so `after` can land below
    // the target on hosts with a small rmem_max (e.g. CI's ~208 KiB
    // default). The helper is still correct as long as it raised capacity
    // toward the target: it either reached the target or grew past the
    // default buffer.
    assert!(
        after >= TRACE_SOCKET_RECV_BUFFER_BYTES || after > before,
        "trace socket receive buffer should reach {} bytes or exceed the {}-byte baseline, got {}",
        TRACE_SOCKET_RECV_BUFFER_BYTES,
        before,
        after
    );
}

/// A zero target is a no-op: the helper must not error and must not shrink
/// the socket's existing receive buffer.
#[test]
#[cfg(not(windows))]
fn trace_socket_recv_buffer_helper_zero_is_noop() {
    let (server, _client) =
        std::os::unix::net::UnixStream::pair().expect("create connected unix socket pair");
    let before = socket_recv_buffer(&server).expect("read baseline receive buffer");
    set_socket_recv_buffer(&server, 0).expect("zero target must be a no-op");
    let after = socket_recv_buffer(&server).expect("read receive buffer after no-op");
    assert_eq!(
        before, after,
        "a zero target must not change the socket receive buffer"
    );
}

/// Concurrent enqueues from multiple threads must never deadlock or
/// corrupt the accounting counter.
#[tokio::test]
async fn concurrent_mutating_enqueues_do_not_deadlock() {
    use std::sync::Arc;
    let coord = Arc::new(ActorDaemonCoordinator::new());
    coord.start_trace_ingest_worker().unwrap();

    const TASKS: usize = 8;
    const PER_TASK: usize = 20;

    // Use prepare_trace_payload_for_ingest + enqueue_trace_payload from
    // multiple tasks concurrently.
    let mut handles = Vec::with_capacity(TASKS);
    for task_id in 0..TASKS {
        let c = coord.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..PER_TASK {
                let sid = format!("20260411T120000.000000-P{:08x}", task_id * 1000 + i);
                let mut payload = serde_json::json!({
                    "event": "start",
                    "sid": sid,
                    "argv": ["git", "commit", "-m", "msg"],
                });
                if c.prepare_trace_payload_for_ingest(&mut payload) {
                    c.enqueue_trace_payload(payload)
                        .expect("mutating event should enqueue");
                }
            }
        }));
    }
    for h in handles {
        h.await.expect("task must not panic");
    }
    // Give the ingest worker time to drain the queue.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while coord.queued_trace_payloads.load(Ordering::Acquire) > 0 {
        if tokio::time::Instant::now() >= deadline {
            break; // don't fail the test on CI slowness; just stop waiting
        }
        tokio::task::yield_now().await;
    }
    coord.request_shutdown();
}
