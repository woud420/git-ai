#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

struct BisectDiagnostics<'a>(&'a TestRepo);

impl Drop for BisectDiagnostics<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let (_, log) = self.0.daemon_diagnostics();
            let lines: Vec<_> = log
                .lines()
                .filter(|line| line.contains("bisect cursor"))
                .collect();
            eprintln!("Bisect cursor diagnostics:\n{}", lines.join("\n"));
            eprintln!(
                "Bisect completions: {:#?}",
                self.0.daemon_completion_entries()
            );
        }
    }
}

fn history() -> (TestRepo, Vec<String>) {
    let repo = TestRepo::new();
    let heads = prepare_history(&repo);
    (repo, heads)
}

fn prepare_history(repo: &TestRepo) -> Vec<String> {
    fs::write(repo.path().join("pending.txt"), "base\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "pending.txt"])
        .unwrap();
    let mut heads = Vec::new();
    for number in 0..8 {
        let marker = format!("revision {number}");
        fs::write(repo.path().join("history.txt"), format!("{marker}\n")).unwrap();
        repo.git_ai(&["checkpoint", "mock_known_human", "history.txt"])
            .unwrap();
        repo.git(&["add", "--all"]).unwrap();
        repo.git(&["commit", "-m", &marker]).unwrap();
        repo.filename("pending.txt")
            .assert_committed_lines(lines!["base".human()]);
        repo.filename("history.txt")
            .assert_committed_lines(lines![marker.human()]);
        heads.push(repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string());
    }
    heads
}

fn pending_ai(repo: &TestRepo) {
    repo.git_ai(&["checkpoint", "human", "pending.txt"])
        .unwrap();
    fs::write(repo.path().join("pending.txt"), "base\npending AI\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "pending.txt"])
        .unwrap();
}

fn assert_pending_commit(repo: &TestRepo) {
    let _diagnostics = BisectDiagnostics(repo);
    assert_eq!(
        fs::read_to_string(repo.path().join("pending.txt")).unwrap(),
        "base\npending AI\n"
    );
    let marker = fs::read_to_string(repo.path().join("history.txt")).unwrap();
    repo.git(&["add", "--all"]).unwrap();
    repo.git(&["commit", "-m", "Commit pending edit after bisect"])
        .unwrap();
    repo.filename("pending.txt")
        .assert_committed_lines(lines!["base".human(), "pending AI".ai()]);
    repo.filename("history.txt")
        .assert_committed_lines(lines![marker.trim().human()]);
}

fn start(repo: &TestRepo, heads: &[String]) {
    repo.git(&["bisect", "start", &heads[7], &heads[0]])
        .unwrap();
    assert_ne!(repo.git(&["rev-parse", "HEAD"]).unwrap().trim(), heads[7]);
}

#[test]
fn bisect_carryover_start_preserves_pending_ai() {
    let (repo, heads) = history();
    pending_ai(&repo);
    start(&repo, &heads);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_good_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    let previous = repo.git(&["rev-parse", "HEAD"]).unwrap();
    pending_ai(&repo);
    repo.git(&["bisect", "good"]).unwrap();
    assert_ne!(repo.git(&["rev-parse", "HEAD"]).unwrap(), previous);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_bad_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    let previous = repo.git(&["rev-parse", "HEAD"]).unwrap();
    pending_ai(&repo);
    repo.git(&["bisect", "bad"]).unwrap();
    assert_ne!(repo.git(&["rev-parse", "HEAD"]).unwrap(), previous);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_reset_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    pending_ai(&repo);
    repo.git(&["bisect", "reset"]).unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]).unwrap().trim(), heads[7]);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_no_checkout_preserves_pending_ai() {
    let (repo, heads) = history();
    pending_ai(&repo);
    repo.git(&["bisect", "start", "--no-checkout", &heads[7], &heads[0]])
        .unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]).unwrap().trim(), heads[7]);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_failed_start_preserves_pending_ai() {
    let (repo, heads) = history();
    pending_ai(&repo);
    assert!(repo.git(&["bisect", "start", "--invalid-option"]).is_err());
    assert_eq!(repo.git(&["rev-parse", "HEAD"]).unwrap().trim(), heads[7]);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_collection_opt_out_preserves_journal() {
    let mut repo = TestRepo::new_dedicated_daemon();
    let heads = prepare_history(&repo);
    pending_ai(&repo);
    let log = repo.current_working_logs();
    let before = fs::read(log.checkpoints_file()).unwrap();
    repo.patch_git_ai_config(|patch| patch.allowed_repositories = Some(Vec::new()));
    start(&repo, &heads);
    repo.sync_daemon();
    assert_eq!(fs::read(log.checkpoints_file()).unwrap(), before);
}

#[test]
fn bisect_carryover_skip_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    let previous = repo.git(&["rev-parse", "HEAD"]).unwrap();
    pending_ai(&repo);
    repo.git(&["bisect", "skip"]).unwrap();
    assert_ne!(repo.git(&["rev-parse", "HEAD"]).unwrap(), previous);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_next_same_head_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    let previous = repo.git(&["rev-parse", "HEAD"]).unwrap();
    pending_ai(&repo);
    repo.git(&["bisect", "next"]).unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]).unwrap(), previous);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_explicit_reset_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    pending_ai(&repo);
    repo.git(&["bisect", "reset", &heads[2]]).unwrap();
    assert_eq!(repo.git(&["rev-parse", "HEAD"]).unwrap().trim(), heads[2]);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_log_preserves_pending_ai() {
    let (repo, heads) = history();
    start(&repo, &heads);
    let previous = repo.git(&["rev-parse", "HEAD"]).unwrap();
    pending_ai(&repo);
    assert!(
        repo.git(&["bisect", "log"])
            .unwrap()
            .contains("git bisect start")
    );
    assert_eq!(repo.git(&["rev-parse", "HEAD"]).unwrap(), previous);
    assert_pending_commit(&repo);
}

fn wait_for_gate(gate: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            Instant::now() < deadline,
            "bisect did not reach its side-effect gate"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn bisect_carryover_delayed_effect_preserves_later_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let gate = dir.path().join("gate");
    fs::write(&gate, "hold").unwrap();
    let spec = format!("bisect={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    let heads = prepare_history(&repo);
    pending_ai(&repo);
    repo.git_without_test_sync_for_test(&["bisect", "start", &heads[7], &heads[0]], &[])
        .unwrap();
    wait_for_gate(&gate);
    fs::write(
        repo.path().join("pending.txt"),
        "base\npending AI\nlater human\n",
    )
    .unwrap();
    let child = repo
        .git_ai_command_without_pre_sync_for_test(
            &["checkpoint", "mock_known_human", "pending.txt"],
            &[],
        )
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    fs::remove_file(&gate).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let marker = fs::read_to_string(repo.path().join("history.txt")).unwrap();
    repo.stage_all_and_commit("Commit after delayed bisect and later checkpoint")
        .unwrap();
    let _diagnostics = BisectDiagnostics(&repo);
    repo.filename("pending.txt").assert_committed_lines(lines![
        "base".human(),
        "pending AI".ai(),
        "later human".human(),
    ]);
    repo.filename("history.txt")
        .assert_committed_lines(lines![marker.trim().human()]);
}

#[test]
fn bisect_carryover_late_trace_uses_each_recorded_checkout() {
    let (repo, heads) = history();
    pending_ai(&repo);
    let dir = tempfile::tempdir().unwrap();
    let trace = dir.path().join("trace.jsonl");
    let env = [("GIT_TRACE2_EVENT", trace.to_str().unwrap())];
    repo.git_without_test_sync_for_test(&["bisect", "start", &heads[7], &heads[0]], &env)
        .unwrap();
    let middle = repo.git_og(&["rev-parse", "HEAD"]).unwrap();
    repo.git_without_test_sync_for_test(&["bisect", "good"], &env)
        .unwrap();
    assert_ne!(repo.git_og(&["rev-parse", "HEAD"]).unwrap(), middle);
    let before = repo.daemon_total_completion_count();
    let mut socket = git_ai::operations::daemon::open_local_socket_stream_with_timeout(
        &repo.daemon_trace_socket_path(),
        Duration::from_secs(5),
    )
    .unwrap();
    socket.write_all(&fs::read(&trace).unwrap()).unwrap();
    socket.flush().unwrap();
    drop(socket);
    repo.wait_for_daemon_total_completion_count(before, before + 2);
    repo.sync_daemon();
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_sqlite_backend_preserves_pending_ai() {
    use git_ai::config::{NotesBackendConfig, NotesBackendKind};
    let repo = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(NotesBackendConfig {
            kind: NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    let heads = prepare_history(&repo);
    pending_ai(&repo);
    start(&repo, &heads);
    assert_pending_commit(&repo);
    assert!(
        repo.git_og(&["notes", "--ref=ai", "list"])
            .unwrap()
            .trim()
            .is_empty()
    );
}

#[test]
fn bisect_carryover_other_worktree_keeps_its_pending_ai() {
    use repos::test_file::TestFile;
    let (repo, heads) = history();
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        other.to_str().unwrap(),
        &heads[7],
    ])
    .unwrap();
    repo.git_ai_from_working_dir(&other, &["checkpoint", "human", "pending.txt"])
        .unwrap();
    fs::write(other.join("pending.txt"), "base\nother AI\n").unwrap();
    repo.git_ai_from_working_dir(&other, &["checkpoint", "mock_ai", "pending.txt"])
        .unwrap();
    pending_ai(&repo);
    start(&repo, &heads);
    assert_pending_commit(&repo);
    repo.git_from_working_dir(&other, &["add", "pending.txt"])
        .unwrap();
    repo.git_from_working_dir(&other, &["commit", "-m", "Commit other worktree edit"])
        .unwrap();
    for (name, expected) in [
        ("pending.txt", lines!["base".human(), "other AI".ai()]),
        ("history.txt", lines!["revision 7".human()]),
    ] {
        let blame = repo
            .git_ai_from_working_dir(&other, &["blame", name])
            .unwrap();
        TestFile::assert_committed_blame_output(&blame, expected);
    }
}

#[test]
fn bisect_carryover_git_process_count_is_independent_of_pending_files() {
    let mut counts = Vec::new();
    for count in [1, 8] {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("spawns.log");
        let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())]);
        let heads = prepare_history(&repo);
        pending_ai(&repo);
        let names: Vec<_> = (1..count).map(|i| format!("pending-{i}.txt")).collect();
        for name in &names {
            repo.git_ai(&["checkpoint", "human", name]).unwrap();
            fs::write(repo.path().join(name), "extra AI\n").unwrap();
            repo.git_ai(&["checkpoint", "mock_ai", name]).unwrap();
        }
        repo.sync_daemon();
        fs::write(&log, "").unwrap();
        start(&repo, &heads);
        repo.sync_daemon();
        counts.push(fs::read_to_string(&log).unwrap().lines().count());
        assert_pending_commit(&repo);
        for name in &names {
            repo.filename(name)
                .assert_committed_lines(lines!["extra AI".ai()]);
        }
    }
    eprintln!("bisect daemon Git process counts for one/eight pending files: {counts:?}");
    assert_eq!(counts[0], counts[1]);
    assert!(counts[0] <= 6);
}

reuse_tests_in_worktree!(bisect_carryover_start_preserves_pending_ai,);

#[test]
fn bisect_carryover_cold_daemon_preserves_pending_ai() {
    let mut repo = TestRepo::new_dedicated_daemon();
    let heads = prepare_history(&repo);
    pending_ai(&repo);
    repo.sync_daemon();
    repo.restart_dedicated_daemon_for_test();
    start(&repo, &heads);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_path_limited_start_from_subdirectory() {
    let (repo, heads) = history();
    pending_ai(&repo);
    let nested = repo.path().join("nested");
    fs::create_dir(&nested).unwrap();
    repo.git_from_working_dir(
        &nested,
        &[
            "bisect",
            "start",
            "--first-parent",
            &heads[7],
            &heads[0],
            "--",
            ":(top)history.txt",
        ],
    )
    .unwrap();
    assert_ne!(repo.git(&["rev-parse", "HEAD"]).unwrap().trim(), heads[7]);
    assert_pending_commit(&repo);
}

#[test]
fn bisect_carryover_preserves_distinct_attribution_kinds() {
    assert_distinct_attribution_kinds(true);
}

#[test]
fn bisect_carryover_distinct_attribution_control_without_bisect() {
    assert_distinct_attribution_kinds(false);
}

fn assert_distinct_attribution_kinds(with_bisect: bool) {
    let (repo, heads) = history();
    pending_ai(&repo);
    fs::write(
        repo.path().join("pending.txt"),
        "base\npending AI\nknown human\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "pending.txt"])
        .unwrap();
    fs::write(
        repo.path().join("pending.txt"),
        "base\npending AI\nknown human\nuntracked\n",
    )
    .unwrap();
    if with_bisect {
        start(&repo, &heads);
    }
    let marker = fs::read_to_string(repo.path().join("history.txt")).unwrap();
    let commit = repo
        .stage_all_and_commit("Commit attribution kinds after bisect")
        .unwrap();
    repo.filename("pending.txt").assert_committed_lines(lines![
        "base".human(),
        "pending AI".ai(),
        "known human".human(),
        "untracked".unattributed_human(),
    ]);
    repo.filename("history.txt")
        .assert_committed_lines(lines![marker.trim().human()]);
    let entries: Vec<_> = commit
        .authorship_log
        .attestations
        .iter()
        .filter(|file| file.file_path == "pending.txt")
        .flat_map(|file| &file.entries)
        .collect();
    assert!(entries.iter().any(|entry| {
        commit
            .authorship_log
            .metadata
            .humans
            .contains_key(&entry.hash)
            && entry.line_ranges.iter().any(|range| range.contains(3))
    }));
    assert!(
        entries
            .iter()
            .all(|entry| entry.line_ranges.iter().all(|range| !range.contains(4)))
    );
}
