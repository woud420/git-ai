#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

// Plumbing-based restacks rewrite commits with `git commit-tree` + `git update-ref`.
// These tests model that generic Git path directly without an external tool.

use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::operations::daemon::open_local_socket_stream_with_timeout;
use git_ai::operations::git::find_repository_in_path;
use git_ai::operations::git::notes_api::read_note;
use git_ai::operations::git::repository::Repository as GitAiRepository;
use repos::test_file::ExpectedLineExt;
use repos::test_repo::{TestRepo, new_daemon_test_sync_session_id, real_git_executable};
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const TRACE_ROOT_REFLOG_START_OFFSETS_FIELD: &str = "git_ai_root_reflog_start_offsets";

fn setup_initial_commit(repo: &TestRepo) {
    let mut readme = repo.filename("README.md");
    readme.set_contents(lines!["# Test Repo"]);
    repo.stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");
}

fn open_repo(repo: &TestRepo) -> GitAiRepository {
    find_repository_in_path(repo.path().to_str().unwrap())
        .expect("failed to open git-ai repository")
}

fn head_sha(repo: &TestRepo) -> String {
    repo.git(&["rev-parse", "HEAD"])
        .expect("rev-parse HEAD should succeed")
        .trim()
        .to_string()
}

fn assert_note_has_ai_for_file(repo: &TestRepo, commit_sha: &str, file_path: &str) {
    let note = repo
        .read_authorship_note(commit_sha)
        .unwrap_or_else(|| panic!("commit {} should have authorship note", &commit_sha[..8]));
    let log = AuthorshipLog::deserialize_from_string(&note).expect("parse authorship note");
    let attestation = log
        .attestations
        .iter()
        .find(|attestation| attestation.file_path == file_path)
        .unwrap_or_else(|| {
            panic!(
                "commit {} should have attestation for {}: {:?}",
                &commit_sha[..8],
                file_path,
                log.attestations
            )
        });
    assert!(
        attestation.entries.iter().any(|entry| {
            let author_id = entry.hash.split("::").next().unwrap_or(&entry.hash);
            log.metadata.sessions.contains_key(author_id)
                || log.metadata.prompts.contains_key(&entry.hash)
        }),
        "commit {} attestation for {} should contain AI entry: {:?}",
        &commit_sha[..8],
        file_path,
        attestation.entries
    );
}

fn ai_attested_lines_for_file(
    log: &AuthorshipLog,
    file_path: &str,
) -> std::collections::BTreeSet<u32> {
    log.attestations
        .iter()
        .find(|attestation| attestation.file_path == file_path)
        .map(|attestation| {
            attestation
                .entries
                .iter()
                .filter(|entry| {
                    let author_id = entry.hash.split("::").next().unwrap_or(&entry.hash);
                    log.metadata.sessions.contains_key(author_id)
                        || log.metadata.prompts.contains_key(&entry.hash)
                })
                .flat_map(|entry| entry.line_ranges.iter().flat_map(|range| range.expand()))
                .collect()
        })
        .unwrap_or_default()
}

fn raw_traced_git(repo: &TestRepo, args: &[&str]) -> String {
    let mut command = Command::new(real_git_executable());
    command.arg("-C").arg(repo.path()).args(args);
    command.env("HOME", repo.test_home_path());
    command.env(
        "GIT_CONFIG_GLOBAL",
        repo.test_home_path().join(".gitconfig"),
    );
    command.env("XDG_CONFIG_HOME", repo.test_home_path().join(".config"));
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env(
        "GIT_TRACE2_EVENT",
        git_ai::operations::daemon::DaemonConfig::trace2_event_target_for_path(
            &repo.daemon_trace_socket_path(),
        ),
    );
    command.env(
        "GIT_TRACE2_EVENT_NESTING",
        std::env::var("GIT_AI_TEST_TRACE2_NESTING").unwrap_or_else(|_| "0".to_string()),
    );

    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to run raw traced git {:?}: {}", args, error));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "raw traced git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        stdout,
        stderr
    );
    if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{}{}", stdout, stderr)
    }
}

fn raw_traced_git_stdin(repo: &TestRepo, args: &[&str], stdin: &str) -> String {
    let mut command = Command::new(real_git_executable());
    command.arg("-C").arg(repo.path()).args(args);
    command.env("HOME", repo.test_home_path());
    command.env(
        "GIT_CONFIG_GLOBAL",
        repo.test_home_path().join(".gitconfig"),
    );
    command.env("XDG_CONFIG_HOME", repo.test_home_path().join(".config"));
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env(
        "GIT_TRACE2_EVENT",
        git_ai::operations::daemon::DaemonConfig::trace2_event_target_for_path(
            &repo.daemon_trace_socket_path(),
        ),
    );
    command.env(
        "GIT_TRACE2_EVENT_NESTING",
        std::env::var("GIT_AI_TEST_TRACE2_NESTING").unwrap_or_else(|_| "0".to_string()),
    );
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .unwrap_or_else(|error| panic!("failed to run raw traced git {:?}: {}", args, error));
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(stdin.as_bytes())
        .expect("write stdin to raw traced git");
    let output = child
        .wait_with_output()
        .unwrap_or_else(|error| panic!("failed to wait for raw traced git {:?}: {}", args, error));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "raw traced git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        stdout,
        stderr
    );
    combined_output(stdout, stderr)
}

fn raw_traced_git_with_session(repo: &TestRepo, args: &[&str], session: &str) -> String {
    let session_arg = format!("git-ai.testSyncSession={session}");
    let mut command = Command::new(real_git_executable());
    command
        .arg("-C")
        .arg(repo.path())
        .arg("-c")
        .arg(&session_arg)
        .args(args);
    command.env("HOME", repo.test_home_path());
    command.env(
        "GIT_CONFIG_GLOBAL",
        repo.test_home_path().join(".gitconfig"),
    );
    command.env("XDG_CONFIG_HOME", repo.test_home_path().join(".config"));
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env(
        "GIT_TRACE2_EVENT",
        git_ai::operations::daemon::DaemonConfig::trace2_event_target_for_path(
            &repo.daemon_trace_socket_path(),
        ),
    );
    command.env(
        "GIT_TRACE2_EVENT_NESTING",
        std::env::var("GIT_AI_TEST_TRACE2_NESTING").unwrap_or_else(|_| "0".to_string()),
    );

    let output = command
        .output()
        .unwrap_or_else(|error| panic!("failed to run raw traced git {:?}: {}", args, error));
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "raw traced git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        stdout,
        stderr
    );
    combined_output(stdout, stderr)
}

fn raw_untraced_git(repo: &TestRepo, args: &[&str]) -> String {
    repo.git_og_with_env(args, &[("GIT_TRACE2_EVENT", "0")])
        .unwrap_or_else(|error| panic!("raw untraced git {:?} failed: {}", args, error))
}

fn raw_git_trace_to_file(repo: &TestRepo, args: &[&str], trace_path: &Path) -> String {
    let output = raw_git_trace_to_file_output(repo, args, trace_path);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "raw traced git {:?} failed\nstdout: {}\nstderr: {}",
        args,
        stdout,
        stderr
    );
    combined_output(stdout, stderr)
}

fn raw_git_trace_to_file_output(repo: &TestRepo, args: &[&str], trace_path: &Path) -> Output {
    let _ = fs::remove_file(trace_path);
    let mut command = Command::new(real_git_executable());
    command.arg("-C").arg(repo.path()).args(args);
    command.env("HOME", repo.test_home_path());
    command.env(
        "GIT_CONFIG_GLOBAL",
        repo.test_home_path().join(".gitconfig"),
    );
    command.env("XDG_CONFIG_HOME", repo.test_home_path().join(".config"));
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_TRACE2_EVENT", trace_path);
    command.env(
        "GIT_TRACE2_EVENT_NESTING",
        std::env::var("GIT_AI_TEST_TRACE2_NESTING").unwrap_or_else(|_| "0".to_string()),
    );

    command
        .output()
        .unwrap_or_else(|error| panic!("failed to run raw traced git {:?}: {}", args, error))
}

fn combined_output(stdout: String, stderr: String) -> String {
    if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{}{}", stdout, stderr)
    }
}

fn replay_trace_file_to_daemon(repo: &TestRepo, trace_path: &Path) {
    let trace = fs::read(trace_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {}", trace_path.display(), error));
    let mut stream = open_local_socket_stream_with_timeout(
        &repo.daemon_trace_socket_path(),
        Duration::from_secs(2),
    )
    .expect("connect to daemon trace socket");
    stream
        .write_all(&trace)
        .expect("write delayed trace payload to daemon");
    stream.flush().expect("flush delayed trace payload");
}

fn replay_trace_payloads_to_daemon(repo: &TestRepo, payloads: &[Value]) {
    let mut stream = open_local_socket_stream_with_timeout(
        &repo.daemon_trace_socket_path(),
        Duration::from_secs(2),
    )
    .expect("connect to daemon trace socket");
    for payload in payloads {
        let line = serde_json::to_string(payload).expect("serialize trace payload");
        stream
            .write_all(line.as_bytes())
            .expect("write trace payload to daemon");
        stream.write_all(b"\n").expect("write trace newline");
    }
    stream.flush().expect("flush trace payloads");
}

fn open_unfinished_mutating_trace_root(
    repo: &TestRepo,
    sid: &str,
) -> git_ai::operations::daemon::DaemonClientStream {
    let mut stream = open_local_socket_stream_with_timeout(
        &repo.daemon_trace_socket_path(),
        Duration::from_secs(2),
    )
    .expect("connect unfinished trace root to daemon");
    let line = serde_json::to_string(&json!({
        "event": "start",
        "sid": sid,
        "argv": ["git", "commit", "-m", "unfinished earlier command"],
        "time_ns": 1u64,
    }))
    .expect("serialize unfinished trace start");
    stream
        .write_all(line.as_bytes())
        .expect("write unfinished trace start");
    stream
        .write_all(b"\n")
        .expect("write unfinished trace newline");
    stream.flush().expect("flush unfinished trace start");
    stream
}

fn daemon_completed_session(repo: &TestRepo, session: &str) -> bool {
    repo.daemon_completion_entries()
        .iter()
        .any(|entry| entry.test_sync_session.as_deref() == Some(session))
}

fn current_reflog_offsets(repo: &TestRepo) -> serde_json::Map<String, Value> {
    let git_dir = repo.path().join(".git").canonicalize().unwrap();
    let mut offsets = serde_json::Map::new();
    let head_log = git_dir.join("logs").join("HEAD");
    if let Ok(metadata) = fs::metadata(&head_log) {
        offsets.insert(
            format!("worktree:{}:HEAD", git_dir.to_string_lossy()),
            json!(metadata.len()),
        );
    }
    let branch = repo.current_branch();
    let branch_ref = format!("refs/heads/{branch}");
    let branch_log = git_dir.join("logs").join(&branch_ref);
    if let Ok(metadata) = fs::metadata(&branch_log) {
        offsets.insert(format!("common:{branch_ref}"), json!(metadata.len()));
    }
    offsets
}

fn commit_tree_rewrite_current_branch(
    repo: &TestRepo,
    branch: &str,
    new_parent: &str,
    message: &str,
) -> (String, String) {
    let old_head = head_sha(repo);
    let tree = repo
        .git(&["rev-parse", &format!("{}^{{tree}}", old_head)])
        .expect("rev-parse HEAD^{tree} should succeed")
        .trim()
        .to_string();

    let new_head = repo
        .git(&["commit-tree", &tree, "-p", new_parent, "-m", message])
        .expect("git commit-tree should succeed")
        .trim()
        .to_string();

    repo.git(&[
        "update-ref",
        &format!("refs/heads/{}", branch),
        &new_head,
        &old_head,
    ])
    .expect("git update-ref should succeed");

    (old_head, new_head)
}

fn commit_tree_from_existing_tree(
    repo: &TestRepo,
    treeish: &str,
    new_parent: &str,
    message: &str,
) -> String {
    let tree = repo
        .git(&["rev-parse", &format!("{}^{{tree}}", treeish)])
        .expect("rev-parse tree should succeed")
        .trim()
        .to_string();

    repo.git(&["commit-tree", &tree, "-p", new_parent, "-m", message])
        .expect("git commit-tree should succeed")
        .trim()
        .to_string()
}

fn plumbing_restack_child_branch(
    repo: &TestRepo,
    branch: &str,
    old_head: &str,
    new_parent: &str,
    message: &str,
) -> String {
    let old_parent = repo
        .git(&["rev-parse", &format!("{}^", old_head)])
        .expect("rev-parse old parent should succeed")
        .trim()
        .to_string();
    let old_grandparent = repo
        .git(&["rev-parse", &format!("{}^", old_parent)])
        .expect("rev-parse old grandparent should succeed")
        .trim()
        .to_string();

    let synthetic_parent = commit_tree_from_existing_tree(repo, new_parent, &old_grandparent, "_");
    let merged_tree = repo
        .git(&[
            "merge-tree",
            "--allow-unrelated-histories",
            &synthetic_parent,
            old_head,
        ])
        .expect("git merge-tree should succeed")
        .trim()
        .to_string();

    let new_head = repo
        .git(&["commit-tree", &merged_tree, "-p", new_parent, "-m", message])
        .expect("git commit-tree for rewritten child should succeed")
        .trim()
        .to_string();

    repo.git(&[
        "update-ref",
        &format!("refs/heads/{}", branch),
        &new_head,
        old_head,
    ])
    .expect("git update-ref should succeed");

    new_head
}

fn delayed_checkout_switch_merge_trace_replay_does_not_attribute_later_uncheckpointed_edit(
    command: &[&str],
) {
    let repo = TestRepo::new();
    let mut file = repo.filename("merge-carry.txt");

    file.set_contents(lines!["one", "two"]);
    repo.stage_all_and_commit("base").unwrap();
    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"]).unwrap();
    fs::write(repo.path().join("merge-carry.txt"), "one feature\ntwo\n").unwrap();
    repo.stage_all_and_commit("feature edit").unwrap();

    repo.git(&["checkout", &default_branch]).unwrap();
    fs::write(repo.path().join("merge-carry.txt"), "one\ntwo ai\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "merge-carry.txt"])
        .unwrap();
    repo.sync_daemon();
    let baseline = repo.daemon_total_completion_count();

    let trace_dir = tempfile::tempdir().expect("trace temp dir");
    let trace = trace_dir.path().join("checkout-switch-merge.trace2");

    raw_git_trace_to_file(&repo, command, &trace);
    fs::write(
        repo.path().join("merge-carry.txt"),
        "one feature\ntwo ai\nlater untracked\n",
    )
    .unwrap();

    replay_trace_file_to_daemon(&repo, &trace);
    repo.wait_for_daemon_total_completion_count(baseline, baseline + 1);

    repo.stage_all_and_commit("commit carried merge").unwrap();
    file.assert_committed_lines(lines![
        "one feature".human(),
        "two ai".ai(),
        "later untracked".ai(),
    ]);
}

#[path = "commit_tree_update_ref/amend_restack.rs"]
mod amend_restack;
#[path = "commit_tree_update_ref/plumbing_rewrites.rs"]
mod plumbing_rewrites;
#[path = "commit_tree_update_ref/sequencer_replay.rs"]
mod sequencer_replay;
#[path = "commit_tree_update_ref/stash_and_checkout.rs"]
mod stash_and_checkout;
#[path = "commit_tree_update_ref/trace_replay.rs"]
mod trace_replay;
#[path = "commit_tree_update_ref/update_ref_attribution.rs"]
mod update_ref_attribution;
