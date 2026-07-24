//! Attribution self-check: exercises a real checkpoint -> commit -> blame
//! round trip in a throwaway repo to verify end-to-end AI/known-human/
//! untracked attribution.

use crate::config::Config;
use crate::diagnostic_sentinels::{
    DEBUG_SELF_CHECK_REMOTE_URL, debug_self_check_root, path_is_in_debug_self_check_root,
};
use crate::operations::commands::blame::GitAiBlameOptions;
use crate::operations::daemon::self_check::{
    CommandRecord, DEBUG_CHECK_TIMEOUT, DiagnosticCheckResult, GitDiagnosticTarget, POLL_INTERVAL,
    remaining_timeout, run_required_until, sanitize_label,
};
use crate::operations::git::repository::discover_repository_in_path_no_git_exec;
use std::fs;
use std::path::Path;
use std::time::Instant;

const SELF_CHECK_FILE: &str = "git-ai-debug-self-check.txt";
const SELF_CHECK_CONTENT_UNTRACKED: &str = "Untracked line\n";
const SELF_CHECK_CONTENT_KNOWN_HUMAN: &str = "Untracked line\nKnown human line\n";
const SELF_CHECK_CONTENT_AI: &str = "Untracked line\nKnown human line\nAI line\n";

pub fn run_attribution_self_check(target: &GitDiagnosticTarget) -> DiagnosticCheckResult {
    let mut commands = Vec::new();
    let deadline = Instant::now() + DEBUG_CHECK_TIMEOUT;
    let repo_path = debug_self_check_root().join(format!(
        "{}-{}",
        sanitize_label(&target.label),
        crate::uuid::generate_v4()
    ));
    let file_path = repo_path.join(SELF_CHECK_FILE);

    let result = (|| -> Result<Vec<String>, String> {
        fs::create_dir_all(&repo_path)
            .map_err(|e| format!("failed to create {}: {}", repo_path.display(), e))?;

        run_required_until(
            &mut commands,
            &target.program,
            &["init", "."],
            Some(&repo_path),
            deadline,
        )?;
        run_required_until(
            &mut commands,
            &target.program,
            &["config", "user.name", "Git AI Debug"],
            Some(&repo_path),
            deadline,
        )?;
        run_required_until(
            &mut commands,
            &target.program,
            &["config", "user.email", "debug-self-check@git-ai.invalid"],
            Some(&repo_path),
            deadline,
        )?;
        run_required_until(
            &mut commands,
            &target.program,
            &["remote", "add", "origin", DEBUG_SELF_CHECK_REMOTE_URL],
            Some(&repo_path),
            deadline,
        )?;

        fs::write(&file_path, SELF_CHECK_CONTENT_UNTRACKED)
            .map_err(|e| format!("failed to write {}: {}", file_path.display(), e))?;
        run_git_ai_checkpoint(&mut commands, &repo_path, "human", deadline)?;
        wait_for_checkpoint_count(&repo_path, 1, deadline)?;

        fs::write(&file_path, SELF_CHECK_CONTENT_KNOWN_HUMAN)
            .map_err(|e| format!("failed to write {}: {}", file_path.display(), e))?;
        run_git_ai_checkpoint(&mut commands, &repo_path, "mock_known_human", deadline)?;
        wait_for_checkpoint_count(&repo_path, 2, deadline)?;

        fs::write(&file_path, SELF_CHECK_CONTENT_AI)
            .map_err(|e| format!("failed to write {}: {}", file_path.display(), e))?;
        run_git_ai_checkpoint(&mut commands, &repo_path, "mock_ai", deadline)?;
        wait_for_checkpoint_count(&repo_path, 3, deadline)?;

        run_required_until(
            &mut commands,
            &target.program,
            &["add", SELF_CHECK_FILE],
            Some(&repo_path),
            deadline,
        )?;
        run_required_until(
            &mut commands,
            &target.program,
            &["commit", "-m", "git-ai debug self check"],
            Some(&repo_path),
            deadline,
        )?;

        let commit_sha = run_required_until(
            &mut commands,
            &target.program,
            &["rev-parse", "HEAD"],
            Some(&repo_path),
            deadline,
        )?
        .stdout
        .trim()
        .to_string();

        let mut details = poll_self_check_attribution(&repo_path, &commit_sha, deadline)?;
        details.insert(0, format!("repo: {}", repo_path.display()));
        details.insert(1, format!("commit: {}", commit_sha));
        details.insert(
            2,
            format!("notes backend: {}", Config::get().notes_backend_kind()),
        );
        Ok(details)
    })();

    match result {
        Ok(details) => {
            let _ = fs::remove_dir_all(&repo_path);
            DiagnosticCheckResult::passed("attribution self-check completed", details, commands)
        }
        Err(err) => {
            let mut details = vec![format!("repo: {}", repo_path.display()), err];
            details.push(
                crate::operations::daemon::control_api::daemon_family_status_detail(&repo_path),
            );
            if path_is_in_debug_self_check_root(&repo_path) {
                details.push(
                    "failed self-check repository was left in place for inspection".to_string(),
                );
            }
            DiagnosticCheckResult::failed("attribution self-check failed", details, commands)
        }
    }
}

fn run_git_ai_checkpoint(
    commands: &mut Vec<CommandRecord>,
    repo_path: &Path,
    preset: &str,
    deadline: Instant,
) -> Result<CommandRecord, String> {
    let git_ai = std::env::current_exe()
        .map_err(|e| format!("failed to resolve git-ai binary path: {}", e))?;
    let git_ai = git_ai.to_string_lossy().to_string();
    run_required_until(
        commands,
        &git_ai,
        &["checkpoint", preset, SELF_CHECK_FILE],
        Some(repo_path),
        deadline,
    )
}

fn wait_for_checkpoint_count(
    repo_path: &Path,
    expected_min_count: usize,
    deadline: Instant,
) -> Result<(), String> {
    let start = Instant::now();
    let mut last_error = None;

    while Instant::now() < deadline {
        match read_checkpoint_count(repo_path) {
            Ok(count) if count >= expected_min_count => return Ok(()),
            Ok(count) => {
                last_error = Some(format!(
                    "only {} checkpoint(s) visible, expected at least {}",
                    count, expected_min_count
                ));
            }
            Err(e) => last_error = Some(e),
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    Err(format!(
        "timed out after {:.1}s waiting for checkpoint persistence: {}",
        start.elapsed().as_secs_f64(),
        last_error.unwrap_or_else(|| {
            format!(
                "no checkpoint status available for repo {}",
                repo_path.display()
            )
        })
    ))
}

fn read_checkpoint_count(repo_path: &Path) -> Result<usize, String> {
    let repo = discover_repository_in_path_no_git_exec(repo_path).map_err(|e| e.to_string())?;
    let working_log = repo
        .storage
        .working_log_for_base_commit("initial")
        .map_err(|e| e.to_string())?;
    working_log
        .read_all_checkpoints()
        .map(|checkpoints| checkpoints.len())
        .map_err(|e| e.to_string())
}

fn poll_self_check_attribution(
    repo_path: &Path,
    commit_sha: &str,
    deadline: Instant,
) -> Result<Vec<String>, String> {
    let start = Instant::now();
    let repo = discover_repository_in_path_no_git_exec(repo_path).map_err(|e| e.to_string())?;
    let notes_backend = Config::get().notes_backend_kind();
    let mut last_error = None;

    while Instant::now() < deadline {
        match crate::operations::commands::blame::validate_self_check_blame_analysis(
            repo.blame_analysis(SELF_CHECK_FILE, &self_check_blame_options(commit_sha))
                .map_err(|e| e.to_string()),
        ) {
            Ok(details) => return Ok(details),
            Err(err) => last_error = Some(err),
        }

        if remaining_timeout(deadline).is_zero() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }

    Err(format!(
        "timed out after {:.1}s waiting for expected attribution via {} backend for {} in {}: {}",
        start.elapsed().as_secs_f64(),
        notes_backend,
        commit_sha,
        repo_path.display(),
        last_error.unwrap_or_else(|| "no blame analysis result available".to_string())
    ))
}

fn self_check_blame_options(commit_sha: &str) -> GitAiBlameOptions {
    GitAiBlameOptions {
        line_ranges: vec![(1, 3)],
        newest_commit: Some(commit_sha.to_string()),
        use_prompt_hashes_as_names: true,
        return_human_authors_as_human: true,
        ..GitAiBlameOptions::default()
    }
}
