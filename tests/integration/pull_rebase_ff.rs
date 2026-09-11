use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::model::repository::bash_history_db::{BashCallEnd, BashCallStart, BashHistoryDatabase};
use git_ai::model::working_log::AgentId;

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::{DaemonTestScope, TestRepo, real_git_executable};
use crate::test_utils::isolated_bash_history_db_path;
use serde_json::json;
use std::collections::{BTreeSet, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

/// Helper struct that provides a local repo with an upstream containing seeded commits.
/// The local repo is initially behind the upstream (no divergence — fast-forward possible).
struct PullTestSetup {
    /// The local clone - initially behind upstream after setup
    local: TestRepo,
    /// The bare upstream repository (kept alive for the duration of the test)
    #[allow(dead_code)]
    upstream: TestRepo,
    /// SHA of the second commit (upstream is ahead by this)
    upstream_sha: String,
}

/// Creates a test setup for fast-forward pull scenarios:
/// 1. Creates upstream (bare) and local (clone) repos
/// 2. Makes an initial commit in local, pushes to upstream
/// 3. Makes a second commit in local, pushes to upstream
/// 4. Resets local back to initial commit (so local is behind upstream)
///
/// After this setup:
/// - upstream has 2 commits
/// - local has 1 commit (behind by 1)
/// - local can `git pull` to fast-forward to the second commit
fn setup_pull_test() -> PullTestSetup {
    let (local, upstream) = TestRepo::new_with_remote();

    // Make initial commit in local and push
    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    let commit = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    let initial_sha = commit.commit_sha;

    // Push initial commit to upstream
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    // Make second commit (simulating remote changes)
    let mut file = local.filename("upstream_file.txt");
    file.set_contents(vec!["content from upstream".to_string()]);
    let commit = local
        .stage_all_and_commit("upstream commit")
        .expect("upstream commit should succeed");

    let upstream_sha = commit.commit_sha;

    // Push second commit to upstream
    local
        .git(&["push", "origin", "HEAD"])
        .expect("push upstream commit should succeed");

    // Reset local back to initial commit (so it's behind upstream)
    local
        .git(&["reset", "--hard", &initial_sha])
        .expect("reset to initial commit should succeed");

    // Verify local is behind
    assert!(
        local.read_file("upstream_file.txt").is_none(),
        "Local should not have upstream_file.txt after reset"
    );

    PullTestSetup {
        local,
        upstream,
        upstream_sha,
    }
}

/// Helper struct for divergent pull scenarios where local has committed changes
/// and upstream has diverged, requiring a real rebase (not fast-forward).
struct DivergentPullTestSetup {
    local: TestRepo,
    #[allow(dead_code)]
    upstream: TestRepo,
    /// SHA of the local AI commit (will get a new SHA after rebase)
    local_ai_commit_sha: String,
}

/// Creates a test setup for divergent pull --rebase scenarios:
/// 1. Creates upstream (bare) and local (clone) repos
/// 2. Makes an initial commit, pushes to upstream
/// 3. Makes a local AI-authored commit
/// 4. Creates a divergent upstream commit (force-pushed)
/// 5. Resets local back to the AI commit
///
/// After this setup:
/// - upstream has diverged from local (initial + upstream_commit)
/// - local has diverged from upstream (initial + ai_commit)
/// - `git pull --rebase` will rebase the AI commit onto the upstream commit
fn setup_divergent_pull_test() -> DivergentPullTestSetup {
    setup_divergent_pull_test_with_daemon_scope(DaemonTestScope::Shared)
}

fn setup_divergent_pull_test_with_daemon_scope(
    daemon_scope: DaemonTestScope,
) -> DivergentPullTestSetup {
    let (local, upstream) = TestRepo::new_with_remote_with_daemon_scope(daemon_scope);

    // Make initial commit and push
    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    let initial = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    // Create a local committed AI-authored change
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.set_contents(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
    local
        .stage_all_and_commit("add AI feature")
        .expect("AI feature commit should succeed");

    let ai_commit_sha = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    let branch = local.current_branch();

    // Create a divergent upstream commit: reset to initial, commit, force-push
    local
        .git(&["reset", "--hard", &initial.commit_sha])
        .expect("reset should succeed");

    let mut upstream_file = local.filename("upstream_change.txt");
    upstream_file.set_contents(vec!["upstream content".to_string()]);
    local
        .stage_all_and_commit("upstream divergent commit")
        .expect("upstream commit should succeed");

    local
        .git(&["push", "--force", "origin", &format!("HEAD:{}", branch)])
        .expect("force push upstream commit should succeed");

    // Reset back to the local AI commit
    local
        .git(&["reset", "--hard", &ai_commit_sha])
        .expect("reset to AI commit should succeed");

    DivergentPullTestSetup {
        local,
        upstream,
        local_ai_commit_sha: ai_commit_sha,
    }
}

/// Creates a setup where local has one AI commit and upstream has an equivalent patch
/// under a different commit hash plus additional upstream commits.
/// A subsequent `pull --rebase` should skip the local commit and not map all upstream history
/// as "new rebased commits".
fn setup_pull_rebase_skip_test() -> (TestRepo, TestRepo, String) {
    let (local, upstream) = TestRepo::new_with_remote();

    // Initial commit and push
    let mut readme = local.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    let initial = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    // Local AI commit (this is the one that should be skipped during pull --rebase)
    let mut ai_file = local.filename("ai_feature.txt");
    ai_file.set_contents(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
    let local_ai = local
        .stage_all_and_commit("local ai commit")
        .expect("local ai commit should succeed");

    let branch = local.current_branch();

    // Simulate upstream history with equivalent patch under a different commit hash.
    // Reset to initial, re-commit same file content with different message, then add extra commits.
    local
        .git(&["reset", "--hard", &initial.commit_sha])
        .expect("reset to initial should succeed");

    ai_file.set_contents(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);
    local
        .stage_all_and_commit("upstream equivalent ai commit")
        .expect("upstream equivalent ai commit should succeed");

    let mut upstream_file = local.filename("upstream_only.txt");
    upstream_file.set_contents(vec!["upstream extra 1".to_string()]);
    local
        .stage_all_and_commit("upstream extra 1")
        .expect("upstream extra 1 should succeed");
    upstream_file.set_contents(vec![
        "upstream extra 1".to_string(),
        "upstream extra 2".to_string(),
    ]);
    local
        .stage_all_and_commit("upstream extra 2")
        .expect("upstream extra 2 should succeed");

    // Force-push divergent upstream state
    local
        .git(&["push", "--force", "origin", &format!("HEAD:{}", branch)])
        .expect("force push upstream state should succeed");

    // Restore local branch to the original local AI commit (now divergent from upstream)
    local
        .git(&["reset", "--hard", &local_ai.commit_sha])
        .expect("reset back to local ai commit should succeed");

    (local, upstream, local_ai.commit_sha)
}

fn insert_bash_recovery_call_covering_now(db_path: &str, repo: &TestRepo) {
    let now_ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let started_at_ns = now_ns.saturating_sub(1_000_000_000);
    let ended_at_ns = now_ns.saturating_add(10_000_000_000);
    let repo_work_dir = repo.canonical_path().to_string_lossy().to_string();
    let agent_id = AgentId {
        tool: "codex".to_string(),
        id: "fast-forward-recovery-session".to_string(),
        model: "gpt-5".to_string(),
    };

    let mut db = BashHistoryDatabase::open_at_path(std::path::Path::new(db_path))
        .expect("bash history db should open");
    db.record_start(&BashCallStart {
        original_cwd: repo_work_dir.clone(),
        repo_work_dir: Some(repo_work_dir.clone()),
        repo_discovery_error: None,
        session_id: agent_id.id.clone(),
        tool_use_id: "fast-forward-recovery-tool-use".to_string(),
        agent_id: agent_id.clone(),
        start_trace_id: "fast-forward-recovery-start".to_string(),
        started_at_ns,
        command: Some("codex exec".to_string()),
        metadata: HashMap::new(),
    })
    .expect("bash call start should insert");
    db.record_end(&BashCallEnd {
        original_cwd: repo_work_dir.clone(),
        repo_work_dir: Some(repo_work_dir),
        repo_discovery_error: None,
        session_id: agent_id.id.clone(),
        tool_use_id: "fast-forward-recovery-tool-use".to_string(),
        agent_id,
        start_trace_id: Some("fast-forward-recovery-start".to_string()),
        end_trace_id: "fast-forward-recovery-end".to_string(),
        started_at_ns: Some(started_at_ns),
        ended_at_ns,
        command: Some("codex exec".to_string()),
        metadata: HashMap::new(),
    })
    .expect("bash call end should insert");
}

// =============================================================================
// Pull --rebase with conflict resolution (notes lost due to SHA rewrite)
// Reproduces: docs/test-rebase-notes-los.md
// =============================================================================

/// Setup for the two-session conflict scenario:
/// Session A and Session B both start from the same commit. Session A pushes
/// AI-authored changes to a shared file. Session B independently makes
/// AI-authored changes to the same file (without pulling). When Session B
/// rebases, the shared file conflicts. After resolution, AI authorship notes
/// should be preserved on the new (rebased) commit SHAs.
struct ConflictPullTestSetup {
    local: TestRepo,
    #[allow(dead_code)]
    upstream: TestRepo,
    /// SHA of Session B's local AI commit (will get a new SHA after rebase)
    session_b_ai_commit_sha: String,
}

/// Creates a test setup that reproduces the notes-lost-on-rebase scenario:
/// 1. Creates upstream (bare) and local (clone) repos
/// 2. Makes an initial commit with a shared file, pushes to upstream
/// 3. Simulates Session A: edits the shared file with AI content, pushes
/// 4. Simulates Session B: resets local to before Session A's push,
///    edits the same shared file with different AI content (diverged)
///
/// After this setup:
/// - upstream has: initial + Session A's AI commit (edits README.md)
/// - local has: initial + Session B's AI commit (edits README.md)
/// - `git pull --rebase` will conflict on README.md
fn setup_conflict_pull_test() -> ConflictPullTestSetup {
    let (local, upstream) = TestRepo::new_with_remote();

    // Initial commit: shared file that both sessions will edit
    let mut readme = local.filename("README.md");
    readme.set_contents(vec![
        "# Project".human(),
        "Initial content line 1".human(),
        "Initial content line 2".human(),
    ]);
    let initial = local
        .stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push initial commit should succeed");

    let branch = local.current_branch();

    // --- Session A: edit README.md with AI content, push to upstream ---
    let mut session_a_readme = local.filename("README.md");
    session_a_readme.set_contents(vec![
        "# Project".human(),
        "Session A: AI-enhanced line 1".ai(),
        "Session A: AI-enhanced line 2".ai(),
    ]);
    local
        .stage_all_and_commit("Session A: AI enhancements")
        .expect("Session A commit should succeed");

    local
        .git(&["push", "origin", &format!("HEAD:{}", branch)])
        .expect("Session A push should succeed");

    // --- Session B: reset to initial (as if Session B never saw A's push) ---
    local
        .git(&["reset", "--hard", &initial.commit_sha])
        .expect("reset to initial should succeed");

    // Session B: edit the same file with different AI content
    let mut session_b_readme = local.filename("README.md");
    session_b_readme.set_contents(vec![
        "# Project".human(),
        "Session B: AI-generated line 1".ai(),
        "Session B: AI-generated line 2".ai(),
    ]);
    local
        .stage_all_and_commit("Session B: AI feature")
        .expect("Session B commit should succeed");

    let session_b_sha = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    ConflictPullTestSetup {
        local,
        upstream,
        session_b_ai_commit_sha: session_b_sha,
    }
}

// =============================================================================
// Regular (non-pull) rebase with conflict scenarios
// =============================================================================

/// Setup for regular rebase conflict tests: local-only repo with a feature branch
/// that has AI commits conflicting with main.
struct RegularRebaseConflictSetup {
    repo: TestRepo,
    /// SHA of the AI commit on the feature branch
    feature_ai_commit_sha: String,
    /// SHA of the conflicting commit on the default branch
    main_conflict_commit_sha: String,
    /// Name of the default branch
    default_branch: String,
}

fn setup_regular_rebase_conflict() -> RegularRebaseConflictSetup {
    let repo = TestRepo::new();

    // Create initial commit with a shared file
    let mut shared_file = repo.filename("shared.txt");
    shared_file.set_contents(vec!["line 1".human(), "line 2".human()]);
    repo.stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    let default_branch = repo.current_branch();

    // Create feature branch with AI-authored changes to the shared file
    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout -b feature should succeed");

    let mut feature_file = repo.filename("shared.txt");
    feature_file.set_contents(vec!["line 1".human(), "AI feature line 2".ai()]);
    repo.stage_all_and_commit("AI feature changes")
        .expect("AI feature commit should succeed");

    let feature_sha = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    // Make conflicting change on main
    repo.git(&["checkout", &default_branch])
        .expect("checkout main should succeed");

    let mut main_file = repo.filename("shared.txt");
    main_file.set_contents(vec!["line 1".human(), "main change line 2".human()]);
    repo.stage_all_and_commit("main conflicting change")
        .expect("main commit should succeed");
    let main_conflict_commit_sha = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    // Switch back to feature
    repo.git(&["checkout", "feature"])
        .expect("checkout feature should succeed");

    RegularRebaseConflictSetup {
        repo,
        feature_ai_commit_sha: feature_sha,
        main_conflict_commit_sha,
        default_branch,
    }
}

fn setup_regular_rebase_conflict_with_trailing_newlines() -> RegularRebaseConflictSetup {
    use std::fs;

    let repo = TestRepo::new();
    let shared_path = repo.path().join("shared.txt");

    repo.human_edit("shared.txt", "line 1\nline 2\n");
    repo.stage_all_and_commit("initial commit")
        .expect("initial commit should succeed");

    let default_branch = repo.current_branch();

    repo.git(&["checkout", "-b", "feature"])
        .expect("checkout -b feature should succeed");

    fs::write(&shared_path, "line 1\nAI feature line 2\n").expect("write feature file");
    repo.git_ai(&["checkpoint", "mock_ai", "shared.txt"])
        .expect("feature AI checkpoint should succeed");
    repo.stage_all_and_commit("AI feature changes")
        .expect("AI feature commit should succeed");

    let feature_sha = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    repo.git(&["checkout", &default_branch])
        .expect("checkout main should succeed");

    repo.human_edit("shared.txt", "line 1\nmain change line 2\n");
    repo.stage_all_and_commit("main conflicting change")
        .expect("main commit should succeed");
    let main_conflict_commit_sha = repo
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse should succeed")
        .trim()
        .to_string();

    repo.git(&["checkout", "feature"])
        .expect("checkout feature should succeed");

    RegularRebaseConflictSetup {
        repo,
        feature_ai_commit_sha: feature_sha,
        main_conflict_commit_sha,
        default_branch,
    }
}

fn session_keys(log: &AuthorshipLog) -> BTreeSet<String> {
    log.metadata.sessions.keys().cloned().collect()
}

fn checkpoint_claude_file_edit(
    repo: &TestRepo,
    event_name: &str,
    file_path: &str,
    tool_use_id: &str,
) {
    let transcript_path = repo.path().join(".git-ai-test-claude-session.jsonl");
    std::fs::write(
        &transcript_path,
        "{\"type\":\"message\",\"message\":{\"model\":\"claude-sonnet-4-5\"}}\n",
    )
    .expect("write claude transcript fixture");
    let absolute_file_path = repo.path().join(file_path);
    let hook_input = json!({
        "cwd": repo.path(),
        "transcript_path": transcript_path,
        "hook_event_name": event_name,
        "tool_name": "Edit",
        "session_id": "test-claude-rebase-conflict-session",
        "tool_use_id": tool_use_id,
        "tool_input": {
            "file_path": absolute_file_path,
        },
    })
    .to_string();

    repo.checkpoint_with_hook_input("claude", &hook_input)
        .expect("claude checkpoint should succeed");
}

fn attestation_author_keys(log: &AuthorshipLog, path: &str) -> BTreeSet<String> {
    log.attestations
        .iter()
        .filter(|attestation| attestation.file_path == path)
        .flat_map(|attestation| attestation.entries.iter())
        .map(|entry| {
            entry
                .hash
                .split("::")
                .next()
                .unwrap_or(&entry.hash)
                .to_string()
        })
        .collect()
}

mod pull_configuration;
mod pull_conflicts;
mod rebase_conflict_recovery;
mod rebase_conflict_sources;

// =============================================================================
// Fast-forward pull tests
// =============================================================================

#[test]
fn test_fast_forward_pull_preserves_ai_attribution() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Create local AI changes (uncommitted)
    let mut ai_file = local.filename("ai_work.txt");
    ai_file.set_contents(vec!["AI generated line 1".ai(), "AI generated line 2".ai()]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Configure git pull behavior for Git 2.52.0+ compatibility
    local
        .git(&["config", "pull.rebase", "false"])
        .expect("config should succeed");
    local
        .git(&["config", "pull.ff", "only"])
        .expect("config should succeed");

    // Perform fast-forward pull
    local.git(&["pull"]).expect("pull should succeed");

    // Commit and verify AI attribution is preserved through the ff pull
    local
        .stage_all_and_commit("commit after pull")
        .expect("commit should succeed");
    ai_file.assert_lines_and_blame(vec!["AI generated line 1".ai(), "AI generated line 2".ai()]);
}

#[test]
fn test_fast_forward_pull_without_local_changes() {
    let setup = setup_pull_test();
    let local = setup.local;

    // Configure git pull behavior
    local
        .git(&["config", "pull.ff", "only"])
        .expect("config should succeed");

    // No local changes - just a clean fast-forward pull
    local.git(&["pull"]).expect("pull should succeed");

    // Verify we got the upstream changes
    assert!(
        local.read_file("upstream_file.txt").is_some(),
        "Should have upstream_file.txt after pull"
    );

    // Verify HEAD is at the expected upstream commit
    let head = local.git(&["rev-parse", "HEAD"]).unwrap();
    assert_eq!(
        head.trim(),
        setup.upstream_sha,
        "HEAD should be at upstream commit"
    );
}

#[test]
fn test_fast_forward_update_ref_bounds_recovery_to_new_tip_parent() {
    let (_bash_db_dir, bash_db_path) = isolated_bash_history_db_path();
    let local = TestRepo::new_with_daemon_env(&[(
        "GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH",
        bash_db_path.as_str(),
    )]);
    let upstream_dir = tempfile::tempdir().expect("upstream temp dir");
    let upstream_path = upstream_dir.path().join("upstream.git");
    let upstream = upstream_path.to_string_lossy().to_string();

    local
        .git_og(&["init", "--bare", &upstream])
        .expect("bare upstream init should succeed");
    local
        .git(&["remote", "add", "origin", &upstream])
        .expect("remote add should succeed");

    std::fs::write(local.path().join("base.txt"), "base\n").expect("write base");
    let old_tip = local
        .stage_all_and_commit("old tip")
        .expect("old tip commit should succeed")
        .commit_sha;
    let branch = local.current_branch();
    local
        .git(&["push", "-u", "origin", "HEAD"])
        .expect("push old tip should succeed");
    local
        .git_og(&[
            "--git-dir",
            &upstream,
            "symbolic-ref",
            "HEAD",
            &format!("refs/heads/{branch}"),
        ])
        .expect("set upstream HEAD should succeed");

    // Seed a working log at the old local tip. The daemon fast-forward
    // update-ref path only finalizes attribution when such a log exists.
    std::fs::write(local.path().join("local_draft.txt"), "local draft\n")
        .expect("write local draft");
    local
        .git_ai(&["checkpoint", "mock_ai", "local_draft.txt"])
        .expect("local checkpoint should succeed");

    let contributor_dir = tempfile::tempdir().expect("contributor temp dir");
    let contributor_path = contributor_dir.path().join("contributor");
    local
        .git_og(&[
            "clone",
            &upstream,
            contributor_path
                .to_str()
                .expect("contributor path should be utf-8"),
        ])
        .expect("contributor clone should succeed");
    let contributor =
        TestRepo::new_at_path_with_daemon_scope(&contributor_path, DaemonTestScope::NoDaemon);

    std::fs::write(
        contributor.path().join("pulled_early_1.txt"),
        "pulled early 1\n",
    )
    .expect("write pulled early 1");
    contributor.git_og(&["add", "-A"]).unwrap();
    contributor
        .git_og(&["commit", "-m", "pulled early 1"])
        .expect("commit pulled early 1 should succeed");
    std::fs::write(
        contributor.path().join("pulled_early_2.txt"),
        "pulled early 2\n",
    )
    .expect("write pulled early 2");
    contributor.git_og(&["add", "-A"]).unwrap();
    contributor
        .git_og(&["commit", "-m", "pulled early 2"])
        .expect("commit pulled early 2 should succeed");
    std::fs::write(contributor.path().join("final_tip.txt"), "final tip\n")
        .expect("write final tip");
    contributor.git_og(&["add", "-A"]).unwrap();
    contributor
        .git_og(&["commit", "-m", "final tip"])
        .expect("commit final tip should succeed");
    contributor
        .git_og(&["push", "origin", &format!("HEAD:{branch}")])
        .expect("push contributor range should succeed");

    local
        .git(&["fetch", "origin", &branch])
        .expect("fetch contributor range should succeed");
    let new_tip = local
        .git(&["rev-parse", "FETCH_HEAD"])
        .expect("rev-parse FETCH_HEAD should succeed")
        .trim()
        .to_string();
    let contributor_final = contributor
        .git_og(&["rev-parse", "HEAD"])
        .expect("rev-parse contributor HEAD should succeed")
        .trim()
        .to_string();
    assert_eq!(new_tip, contributor_final);
    assert_ne!(old_tip, new_tip);

    // `git update-ref` moves the ref but does not update the worktree. Put the
    // files in the worktree so timestamp-based recovery can run deterministically
    // and prove which committed hunks it was allowed to inspect.
    std::fs::write(local.path().join("pulled_early_1.txt"), "pulled early 1\n")
        .expect("write local pulled early 1");
    std::fs::write(local.path().join("pulled_early_2.txt"), "pulled early 2\n")
        .expect("write local pulled early 2");
    std::fs::write(local.path().join("final_tip.txt"), "final tip\n")
        .expect("write local final tip");

    insert_bash_recovery_call_covering_now(&bash_db_path, &local);
    local
        .git(&[
            "update-ref",
            &format!("refs/heads/{branch}"),
            &new_tip,
            &old_tip,
        ])
        .expect("fast-forward update-ref should succeed");

    let new_head = local
        .git(&["rev-parse", "HEAD"])
        .expect("rev-parse HEAD should succeed")
        .trim()
        .to_string();
    assert_eq!(new_head, new_tip);

    let log = local.require_authorship_log(&new_head);
    let attested_files: BTreeSet<String> = log
        .attestations
        .iter()
        .map(|attestation| attestation.file_path.clone())
        .collect();

    assert!(
        attested_files.contains("final_tip.txt"),
        "recovery should still see the finalized tip commit"
    );
    assert!(
        !attested_files.contains("pulled_early_1.txt"),
        "recovery must not diff the whole old_tip..new_head range"
    );
    assert!(
        !attested_files.contains("pulled_early_2.txt"),
        "recovery must not attribute earlier pulled commits to the local session"
    );
}

// =============================================================================
// Pull --rebase --autostash with uncommitted changes
// =============================================================================

#[test]
fn test_pull_rebase_autostash_preserves_uncommitted_ai_attribution() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Add uncommitted AI changes on top of the committed ones
    let mut uncommitted_ai = local.filename("uncommitted_ai.txt");
    uncommitted_ai.set_contents(vec![
        "AI generated line 1".ai(),
        "AI generated line 2".ai(),
        "AI generated line 3".ai(),
    ]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash: uncommitted changes get stashed/unstashed
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit the previously-uncommitted changes
    local
        .stage_all_and_commit("commit after rebase pull")
        .expect("commit should succeed");

    uncommitted_ai.assert_lines_and_blame(vec![
        "AI generated line 1".ai(),
        "AI generated line 2".ai(),
        "AI generated line 3".ai(),
    ]);
}

#[test]
fn test_pull_rebase_autostash_with_mixed_attribution() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Create local uncommitted changes with mixed human and AI attribution
    let mut mixed_file = local.filename("mixed_work.txt");
    mixed_file.set_contents(vec![
        "Human written line 1".human(),
        "AI generated line 1".ai(),
        "Human written line 2".human(),
        "AI generated line 2".ai(),
    ]);

    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit and verify mixed attribution is preserved
    local
        .stage_all_and_commit("commit with mixed attribution")
        .expect("commit should succeed");

    mixed_file.assert_lines_and_blame(vec![
        "Human written line 1".human(),
        "AI generated line 1".ai(),
        "Human written line 2".human(),
        "AI generated line 2".ai(),
    ]);
}

// =============================================================================
// Pull --rebase with both committed AND uncommitted changes
// =============================================================================

#[test]
fn test_pull_rebase_committed_and_autostash_preserves_all_authorship() {
    let setup = setup_divergent_pull_test();
    let local = setup.local;

    // Add uncommitted AI changes on top of the committed AI commit
    let mut uncommitted_ai = local.filename("uncommitted_ai.txt");
    uncommitted_ai.set_contents(vec!["Uncommitted AI line".ai()]);
    local
        .git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Pull --rebase --autostash: committed changes get rebased, uncommitted get stashed
    local
        .git(&["pull", "--rebase", "--autostash"])
        .expect("pull --rebase --autostash should succeed");

    // Commit the previously-uncommitted changes
    local
        .stage_all_and_commit("commit uncommitted AI work")
        .expect("commit should succeed");

    // Verify committed AI authorship survived the rebase
    let mut committed_ai = local.filename("ai_feature.txt");
    committed_ai.assert_lines_and_blame(vec![
        "AI generated feature line 1".ai(),
        "AI generated feature line 2".ai(),
    ]);

    // Verify uncommitted AI authorship survived the autostash cycle
    uncommitted_ai.assert_lines_and_blame(vec!["Uncommitted AI line".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_fast_forward_pull_preserves_ai_attribution,
    test_fast_forward_pull_without_local_changes,
    test_pull_rebase_autostash_preserves_uncommitted_ai_attribution,
    test_pull_rebase_autostash_with_mixed_attribution,
    test_pull_rebase_committed_and_autostash_preserves_all_authorship,
);
