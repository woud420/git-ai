#![cfg(unix)]

// Regression coverage for ENG-322 and upstream git-ai-project/git-ai#1294.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::{TestRepo, real_git_executable};
use crate::repos::write_executable_script;
use crate::test_utils::extract_json_object;
use std::fs;
use std::path::PathBuf;

const NON_C_LOCALE: &str = "git_ai_test_non_c_locale";

struct RecordingGit {
    log_path: PathBuf,
    real_git: String,
}

fn install_recording_git(repo: &TestRepo) -> RecordingGit {
    let wrapper_path = repo.test_home_path().join("recording-locale-git");
    let log_path = repo.test_home_path().join("internal-git-locale.log");
    write_executable_script(
        &wrapper_path,
        r#"#!/bin/sh
printf 'LC_ALL=%s\targv=%s\n' "${LC_ALL-<unset>}" "$*" >> "$GIT_AI_LOCALE_LOG"

is_numstat=0
is_notes_fetch=0
is_machine_log=0
is_cat_file=0
is_batch=0
for arg in "$@"; do
  case "$arg" in
    --numstat) is_numstat=1 ;;
    +refs/notes/ai:*) is_notes_fetch=1 ;;
    --format=format:*) is_machine_log=1 ;;
    cat-file) is_cat_file=1 ;;
    --batch) is_batch=1 ;;
  esac
done

if [ "${GIT_AI_LOCALE_TARGET-}" = "numstat" ] && [ "$is_numstat" = "1" ] && [ "${LC_ALL-}" != "C" ]; then
  printf 'machine numstat used LC_ALL=%s instead of C\n' "${LC_ALL-<unset>}" >&2
  exit 97
fi

if [ "${GIT_AI_LOCALE_TARGET-}" = "fetch-notes" ] && [ "$is_notes_fetch" = "1" ] && [ "${LC_ALL-}" != "C" ]; then
  printf 'fatal: impossible de trouver la reference distante refs/notes/ai\n' >&2
  exit 128
fi

if [ "${GIT_AI_LOCALE_TARGET-}" = "machine-log" ] && [ "$is_machine_log" = "1" ] && [ "${LC_ALL-}" != "C" ]; then
  printf 'machine log used LC_ALL=%s instead of C\n' "${LC_ALL-<unset>}" >&2
  exit 97
fi

if [ "${GIT_AI_LOCALE_TARGET-}" = "cat-file" ] && [ "$is_cat_file" = "1" ] && [ "$is_batch" = "1" ] && [ "${LC_ALL-}" != "C" ]; then
  printf 'machine cat-file used LC_ALL=%s instead of C\n' "${LC_ALL-<unset>}" >&2
  exit 97
fi

exec "$GIT_AI_REAL_GIT" "$@"
"#,
    )
    .expect("write recording Git wrapper");

    // Update only the direct CLI fixture's file-backed config. Using
    // `patch_git_ai_config` here would restart a dedicated daemon with this
    // wrapper before the per-command recording environment exists.
    let config_path = repo.test_home_path().join(".git-ai").join("config.json");
    let mut config: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&config_path).expect("read TestRepo Git config"))
            .expect("parse TestRepo Git config");
    config["git_path"] = serde_json::Value::String(wrapper_path.to_string_lossy().into_owned());
    fs::write(
        &config_path,
        serde_json::to_vec(&config).expect("serialize TestRepo Git config"),
    )
    .expect("write recording Git config");

    RecordingGit {
        log_path,
        real_git: real_git_executable().to_string(),
    }
}

fn run_git_ai_with_locale(
    repo: &TestRepo,
    recording_git: &RecordingGit,
    target: &str,
    args: &[&str],
) -> Result<String, String> {
    fs::write(&recording_git.log_path, "").expect("clear locale log");
    let log_path = recording_git.log_path.to_string_lossy();
    repo.git_ai_with_env(
        args,
        &[
            ("LC_ALL", NON_C_LOCALE),
            ("LANG", NON_C_LOCALE),
            ("GIT_AI_LOCALE_TARGET", target),
            ("GIT_AI_LOCALE_LOG", log_path.as_ref()),
            ("GIT_AI_REAL_GIT", recording_git.real_git.as_str()),
        ],
    )
}

fn recorded_lines(recording_git: &RecordingGit) -> Vec<String> {
    fs::read_to_string(&recording_git.log_path)
        .expect("recording Git should write a locale log")
        .lines()
        .map(str::to_string)
        .collect()
}

fn assert_all_recorded_commands_use(recording_git: &RecordingGit, locale: &str) {
    let lines = recorded_lines(recording_git);
    assert!(
        !lines.is_empty(),
        "expected at least one recorded Git command"
    );
    let prefix = format!("LC_ALL={locale}\t");
    assert!(
        lines.iter().all(|line| line.starts_with(&prefix)),
        "expected every machine-consumed Git command to use {locale}, got:\n{}",
        lines.join("\n")
    );
}

fn assert_recorded_command_uses(recording_git: &RecordingGit, argv_fragment: &str, locale: &str) {
    let lines = recorded_lines(recording_git);
    let matching: Vec<&String> = lines
        .iter()
        .filter(|line| line.contains(argv_fragment))
        .collect();
    assert!(
        !matching.is_empty(),
        "expected a recorded Git command containing {argv_fragment:?}, got:\n{}",
        lines.join("\n")
    );
    let prefix = format!("LC_ALL={locale}\t");
    assert!(
        matching.iter().all(|line| line.starts_with(&prefix)),
        "expected {argv_fragment:?} to use {locale}, got:\n{}",
        matching
            .iter()
            .map(|line| line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn two_commit_repo() -> (TestRepo, String, String) {
    let repo = TestRepo::new();
    let mut file = repo.filename("range.txt");

    fs::write(repo.path().join("range.txt"), "base\n").expect("write range base");
    let first = repo
        .stage_all_and_commit("range base")
        .expect("commit range base");
    file.assert_committed_lines(crate::lines!["base".unattributed_human()]);

    fs::write(repo.path().join("range.txt"), "base\naddition\n").expect("write range addition");
    let second = repo
        .stage_all_and_commit("range addition")
        .expect("commit range addition");
    file.assert_committed_lines(crate::lines![
        "base".unattributed_human(),
        "addition".unattributed_human(),
    ]);

    (repo, first.commit_sha, second.commit_sha)
}

#[test]
fn test_machine_consumed_status_forces_c_locale() {
    let repo = TestRepo::new();
    let mut file = repo.filename("status.txt");
    fs::write(repo.path().join("status.txt"), "base\n").expect("write status base");
    repo.stage_all_and_commit("status base")
        .expect("commit status base");
    file.assert_committed_lines(crate::lines!["base".unattributed_human()]);

    repo.git_ai(&["checkpoint", "human", "status.txt"])
        .expect("capture pre-edit status checkpoint");
    fs::write(repo.path().join("status.txt"), "base\nAI addition\n")
        .expect("write status addition");
    repo.git_ai(&["checkpoint", "mock_ai", "status.txt"])
        .expect("capture AI status checkpoint");

    let recording_git = install_recording_git(&repo);
    let raw = run_git_ai_with_locale(&repo, &recording_git, "numstat", &["status", "--json"])
        .expect("status should succeed when internal Git forces the C locale");
    let json: serde_json::Value =
        serde_json::from_str(&extract_json_object(&raw)).expect("valid status JSON");
    assert_eq!(json["stats"]["git_diff_added_lines"], 1);
    assert_eq!(json["stats"]["ai_accepted"], 1);
    assert_recorded_command_uses(&recording_git, "--numstat", "C");
    assert_all_recorded_commands_use(&recording_git, "C");
}

#[test]
fn test_machine_consumed_range_stats_force_c_locale() {
    let (repo, first, second) = two_commit_repo();
    let recording_git = install_recording_git(&repo);
    let range = format!("{first}..{second}");
    let raw = run_git_ai_with_locale(
        &repo,
        &recording_git,
        "numstat",
        &["stats", &range, "--json"],
    )
    .expect("range stats should succeed when internal Git forces the C locale");
    let json: serde_json::Value =
        serde_json::from_str(&extract_json_object(&raw)).expect("valid range stats JSON");
    assert_eq!(json["range_stats"]["git_diff_added_lines"], 1);
    assert_recorded_command_uses(&recording_git, "--numstat", "C");
    assert_all_recorded_commands_use(&recording_git, "C");
}

#[test]
fn test_machine_consumed_fetch_notes_forces_c_locale() {
    let (mirror, _upstream) = TestRepo::new_with_remote();
    let mut file = mirror.filename("notes.txt");
    fs::write(mirror.path().join("notes.txt"), "notes base\n").expect("write notes base");
    mirror
        .stage_all_and_commit("notes base")
        .expect("commit notes base");
    file.assert_committed_lines(crate::lines!["notes base".unattributed_human()]);

    let recording_git = install_recording_git(&mirror);
    let raw = run_git_ai_with_locale(
        &mirror,
        &recording_git,
        "fetch-notes",
        &["fetch-notes", "--json"],
    )
    .expect("missing remote notes should remain a non-error under a non-C user locale");
    let json: serde_json::Value =
        serde_json::from_str(&extract_json_object(&raw)).expect("valid fetch-notes JSON");
    assert_eq!(json["status"], "not_found");
    assert_recorded_command_uses(&recording_git, "refs/notes/ai:", "C");
    assert_all_recorded_commands_use(&recording_git, "C");
}

#[test]
fn test_user_facing_plain_log_preserves_non_c_locale() {
    let (repo, _first, _second) = two_commit_repo();
    let recording_git = install_recording_git(&repo);
    let raw = run_git_ai_with_locale(
        &repo,
        &recording_git,
        "passthrough",
        &["log", "--no-pager", "--plain", "-n", "1"],
    )
    .expect("plain log passthrough should succeed");
    assert!(
        raw.contains("range addition"),
        "unexpected log output: {raw}"
    );
    assert_recorded_command_uses(&recording_git, " log ", NON_C_LOCALE);
}

#[test]
fn test_machine_consumed_rendered_log_forces_c_locale() {
    let (repo, _first, _second) = two_commit_repo();
    let recording_git = install_recording_git(&repo);
    let raw = run_git_ai_with_locale(
        &repo,
        &recording_git,
        "machine-log",
        &["log", "--no-pager", "-n", "1"],
    )
    .expect("rendered log should succeed when internal Git forces the C locale");
    assert!(
        raw.contains("range addition"),
        "unexpected log output: {raw}"
    );
    assert_recorded_command_uses(&recording_git, "--format=format:", "C");
    assert_all_recorded_commands_use(&recording_git, "C");
}

#[test]
fn test_machine_consumed_notes_migration_forces_c_locale() {
    let repo = TestRepo::new();
    let mut file = repo.filename("note.txt");
    file.set_contents(crate::lines!["AI-authored note".ai()]);
    repo.stage_all_and_commit("note for migration")
        .expect("commit note fixture");
    file.assert_committed_lines(crate::lines!["AI-authored note".ai()]);

    let recording_git = install_recording_git(&repo);
    run_git_ai_with_locale(
        &repo,
        &recording_git,
        "cat-file",
        &["notes", "migrate", "--to", "sqlite"],
    )
    .expect("notes migration should succeed when internal Git forces the C locale");
    assert_recorded_command_uses(&recording_git, "cat-file --batch", "C");
    assert_all_recorded_commands_use(&recording_git, "C");
}

#[test]
fn test_range_stats_normalize_c_quoted_nfd_ignored_path() {
    const NFD_PATH: &str = "cafe\u{0301}.txt";
    const NFC_PATH: &str = "caf\u{00e9}.txt";
    const KEPT_PATH: &str = "kept.txt";

    let repo = TestRepo::new();
    repo.git_og(&["config", "core.precomposeUnicode", "false"])
        .expect("disable Git precomposition");
    repo.git_og(&["config", "core.quotePath", "true"])
        .expect("force C-quoted Git paths");

    fs::write(repo.path().join(".git-ai-ignore"), format!("{NFC_PATH}\n"))
        .expect("write ignore file");
    fs::write(repo.path().join(NFD_PATH), "ignored base\n").expect("write ignored base");
    fs::write(repo.path().join(KEPT_PATH), "kept base\n").expect("write kept base");
    let first = repo
        .stage_all_and_commit("unicode range base")
        .expect("commit unicode range base");

    let mut ignore_file = repo.filename(".git-ai-ignore");
    let mut ignored_file = repo.filename(NFD_PATH);
    let mut kept_file = repo.filename(KEPT_PATH);
    ignore_file.assert_committed_lines(crate::lines![NFC_PATH.unattributed_human()]);
    ignored_file.assert_committed_lines(crate::lines!["ignored base".unattributed_human()]);
    kept_file.assert_committed_lines(crate::lines!["kept base".unattributed_human()]);

    let indexed_paths = repo
        .git_og(&["-c", "core.quotePath=false", "ls-files"])
        .expect("list indexed paths without quoting");
    assert!(
        indexed_paths.lines().any(|path| path == NFD_PATH),
        "fixture must preserve the decomposed path in Git's index: {indexed_paths:?}"
    );

    fs::write(
        repo.path().join(NFD_PATH),
        "ignored base\nignored addition\n",
    )
    .expect("write ignored addition");
    fs::write(repo.path().join(KEPT_PATH), "kept base\nkept addition\n")
        .expect("write kept addition");
    let second = repo
        .stage_all_and_commit("unicode range additions")
        .expect("commit unicode range additions");
    ignore_file.assert_committed_lines(crate::lines![NFC_PATH.unattributed_human()]);
    ignored_file.assert_committed_lines(crate::lines![
        "ignored base".unattributed_human(),
        "ignored addition".unattributed_human(),
    ]);
    kept_file.assert_committed_lines(crate::lines![
        "kept base".unattributed_human(),
        "kept addition".unattributed_human(),
    ]);

    let range = format!("{}..{}", first.commit_sha, second.commit_sha);
    let quoted_numstat = repo
        .git_og_with_env(
            &["diff", "--numstat", &range],
            &[("LC_ALL", "C"), ("LANG", "C")],
        )
        .expect("read C-locale numstat fixture");
    assert!(
        quoted_numstat.contains("\\314\\201"),
        "fixture must contain a C-quoted decomposed path: {quoted_numstat:?}"
    );

    let raw = repo
        .git_ai_with_env(
            &["stats", &range, "--json"],
            &[("LC_ALL", NON_C_LOCALE), ("LANG", NON_C_LOCALE)],
        )
        .expect("range stats should succeed under a non-C user locale");
    let json: serde_json::Value =
        serde_json::from_str(&extract_json_object(&raw)).expect("valid range stats JSON");
    assert_eq!(
        json["range_stats"]["git_diff_added_lines"], 1,
        "the C-quoted, canonically equivalent ignored path must not be counted"
    );
    assert_eq!(json["range_stats"]["git_diff_deleted_lines"], 0);
}

crate::reuse_tests_in_worktree!(test_range_stats_normalize_c_quoted_nfd_ignored_path,);
