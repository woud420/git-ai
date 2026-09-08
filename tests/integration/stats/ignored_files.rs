use super::{
    Command, CommitStats, ExpectedLineExt, TestRepo, configure_repo_external_diff_helper,
    extract_json_object, fs, raw_git, stats_from_args,
};

#[test]
fn test_stats_cli_range_ignores_repo_external_diff_helper() {
    let repo = TestRepo::new();

    let mut file = repo.filename("stats-range-ext.txt");
    file.set_contents(crate::lines!["base".human()]);
    let first = repo.stage_all_and_commit("initial").unwrap();

    file.set_contents(crate::lines!["base".human(), "ai line".ai()]);
    let second = repo.stage_all_and_commit("ai second").unwrap();

    let marker = configure_repo_external_diff_helper(
        &repo,
        "STATS_EXTERNAL_DIFF_MARKER",
        "stats-ext-diff-helper.sh",
    );
    let proxied_diff = repo
        .git(&["diff", &first.commit_sha, &second.commit_sha])
        .expect("proxied git diff should succeed");
    assert!(
        proxied_diff.contains(&marker),
        "sanity check: proxied git diff should use configured external helper"
    );

    let range = format!("{}..{}", first.commit_sha, second.commit_sha);
    let raw = repo
        .git_ai(&["stats", &range, "--json"])
        .expect("git-ai stats range should succeed with external diff configured");
    assert!(
        !raw.contains(&marker),
        "git-ai stats output should not include external helper output, got:\n{}",
        raw
    );

    let output = extract_json_object(&raw);
    let stats: git_ai::operations::authorship::range_authorship::RangeAuthorshipStats =
        serde_json::from_str(&output).unwrap();
    assert_eq!(stats.authorship_stats.total_commits, 1);
    assert!(
        stats.range_stats.git_diff_added_lines >= 1,
        "expected at least one added line in range, got {}",
        stats.range_stats.git_diff_added_lines
    );
    assert!(stats.range_stats.ai_additions >= 1);
}

#[test]
fn test_stats_default_ignores_snapshot_files() {
    let repo = TestRepo::new();
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    repo.filename("src/main.rs")
        .set_contents(crate::lines!["fn main() {}".ai()]);
    repo.filename("__snapshots__/main.snap")
        .set_contents(crate::lines![
            "snapshot line 1",
            "snapshot line 2",
            "snapshot line 3"
        ]);
    repo.stage_all_and_commit("Add source and snapshot")
        .unwrap();

    let stats = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(stats.git_diff_added_lines, 1);
    assert_eq!(stats.ai_additions, 1);
}

#[test]
fn test_stats_default_ignores_lockfiles_and_generated_files() {
    let repo = TestRepo::new();
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    repo.filename("src/lib.rs")
        .set_contents(crate::lines!["pub fn answer() -> u32 { 42 }".ai()]);
    repo.filename("Cargo.lock")
        .set_contents(vec!["lock".to_string().repeat(5); 650]);
    repo.filename("api.generated.ts")
        .set_contents(vec!["export type X = string;".to_string(); 500]);
    repo.stage_all_and_commit("Add source and generated artifacts")
        .unwrap();

    let stats = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(stats.git_diff_added_lines, 1);
    assert_eq!(stats.ai_additions, 1);
}

#[test]
fn test_stats_ignores_linguist_generated_patterns() {
    let repo = TestRepo::new();
    repo.filename(".gitattributes")
        .set_contents(crate::lines!["generated/** linguist-generated=true"]);
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit with gitattributes")
        .unwrap();

    repo.filename("src/main.rs")
        .set_contents(crate::lines!["fn run() {}".ai()]);
    repo.filename("generated/schema.ts")
        .set_contents(crate::lines!["export const schema = {};"]);
    repo.stage_all_and_commit("Add source and linguist-generated file")
        .unwrap();

    let stats = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(stats.git_diff_added_lines, 1);
    assert_eq!(stats.ai_additions, 1);
}

#[test]
fn test_stats_keeps_negative_linguist_patterns_counted() {
    let repo = TestRepo::new();
    repo.filename(".gitattributes").set_contents(crate::lines![
        "generated/** linguist-generated=true",
        "manual/** linguist-generated=false"
    ]);
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit with attrs")
        .unwrap();

    repo.filename("generated/out.ts")
        .set_contents(crate::lines!["export const ignored = true;"]);
    repo.filename("manual/kept.ts")
        .set_contents(crate::lines!["export const counted = true;".ai()]);
    repo.stage_all_and_commit("Add generated and manual files")
        .unwrap();

    let stats = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(stats.git_diff_added_lines, 1);
    assert_eq!(stats.ai_additions, 1);
}

#[test]
fn test_stats_in_bare_clone_uses_root_gitattributes_linguist_generated() {
    let repo = TestRepo::new();
    repo.filename(".gitattributes")
        .set_contents(crate::lines!["generated/** linguist-generated=true"]);
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit with gitattributes")
        .unwrap();

    repo.filename("src/main.rs")
        .set_contents(crate::lines!["fn run() {}".ai()]);
    repo.filename("generated/schema.ts")
        .set_contents(crate::lines!["export const schema = {};"]);
    repo.stage_all_and_commit("Add source and linguist-generated file")
        .unwrap();

    let temp = tempfile::tempdir().expect("tempdir");
    let bare = temp.path().join("repo.git");
    raw_git(
        temp.path(),
        &[
            "clone",
            "--bare",
            repo.path().to_str().unwrap(),
            bare.to_str().unwrap(),
        ],
    );

    let output = Command::new(crate::repos::test_repo::get_binary_path())
        .args(["stats", "HEAD", "--json"])
        .current_dir(&bare)
        .env(
            "GIT_AI_TEST_DB_PATH",
            temp.path().join("db").to_str().unwrap(),
        )
        .output()
        .expect("git-ai stats should run in bare repo");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "git-ai stats failed in bare clone:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );

    let combined = if stdout.is_empty() {
        stderr.to_string()
    } else if stderr.is_empty() {
        stdout.to_string()
    } else {
        format!("{}{}", stdout, stderr)
    };
    let json = extract_json_object(&combined);
    let stats: CommitStats = serde_json::from_str(&json).expect("valid stats json");
    assert_eq!(stats.git_diff_added_lines, 1);
}

#[test]
fn test_stats_ignore_flag_is_additive_to_defaults() {
    let repo = TestRepo::new();
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    repo.filename("src/main.rs")
        .set_contents(crate::lines!["fn main() {}".ai()]);
    repo.filename("docs/keep.txt")
        .set_contents(crate::lines!["this line is human"]);
    repo.stage_all_and_commit("Add docs and source").unwrap();

    let baseline = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(baseline.git_diff_added_lines, 2);

    let ignored = stats_from_args(
        &repo,
        &["stats", "HEAD", "--json", "--ignore", "docs/keep.txt"],
    );
    assert_eq!(ignored.git_diff_added_lines, 1);
}

#[test]
fn test_stats_range_uses_default_ignores() {
    let repo = TestRepo::new();
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    let first = repo.stage_all_and_commit("Initial commit").unwrap();

    repo.filename("src/main.rs")
        .set_contents(crate::lines!["fn main() {}".ai()]);
    repo.filename("Cargo.lock")
        .set_contents(vec!["lockdata".to_string(); 700]);
    let second = repo
        .stage_all_and_commit("Add source and lockfile")
        .unwrap();

    let range = format!("{}..{}", first.commit_sha, second.commit_sha);
    let raw = repo
        .git_ai(&["stats", &range, "--json"])
        .expect("git-ai stats range should succeed");
    let json = extract_json_object(&raw);
    let range_stats: git_ai::operations::authorship::range_authorship::RangeAuthorshipStats =
        serde_json::from_str(&json).unwrap();

    assert_eq!(range_stats.range_stats.git_diff_added_lines, 1);
    assert_eq!(range_stats.range_stats.ai_additions, 1);
}

#[test]
fn test_post_commit_large_ignored_files_do_not_trigger_skip_warning() {
    let repo = TestRepo::new();
    repo.filename("README.md")
        .set_contents(crate::lines!["# Repo"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    repo.filename("Cargo.lock")
        .set_contents(vec!["lockfile-entry".to_string(); 7001]);
    let commit = repo
        .stage_all_and_commit("Large lockfile update")
        .expect("commit should succeed");

    assert!(
        !commit
            .stdout
            .contains("Skipped git-ai stats for large commit"),
        "large ignored files should not trigger post-commit skip warning: {}",
        commit.stdout
    );

    let stats = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(stats.git_diff_added_lines, 0);
    assert_eq!(stats.ai_additions, 0);
    assert_eq!(stats.human_additions, 0);
}

#[test]
fn test_stats_ignores_renamed_files() {
    // Test that stats correctly ignores pure renames (no content changes)
    // Reproduces issue #923
    let repo = TestRepo::new();

    // Initial commit with files in a directory
    repo.filename("misc/Development Notes.md")
        .set_contents(crate::lines![
            "# Development Notes",
            "",
            "Some content here",
            "More content",
            "Even more",
            "Line 6",
            "Line 7",
            "Line 8",
            "Line 9",
            "Line 10",
            "Line 11",
            "Line 12",
            "Line 13",
            "Line 14",
            "Line 15",
            "Line 16"
        ]);
    repo.filename("misc/Usage Guide.md")
        .set_contents(crate::lines!["# Usage Guide", "", "Usage info"]);
    repo.stage_all_and_commit("Initial commit with misc directory")
        .unwrap();

    // Rename the directory (pure rename, no content changes)
    let misc_dev = repo.path().join("misc/Development Notes.md");
    let misc_usage = repo.path().join("misc/Usage Guide.md");
    let new_dir = repo.path().join("Misc Docs");
    fs::create_dir(&new_dir).unwrap();
    fs::rename(&misc_dev, new_dir.join("Development Notes.md")).unwrap();
    fs::rename(&misc_usage, new_dir.join("Usage Guide.md")).unwrap();
    fs::remove_dir(repo.path().join("misc")).unwrap();

    repo.stage_all_and_commit("Rename misc to Misc Docs")
        .unwrap();

    // Verify that git ai diff recognizes this as a rename
    let diff_output = repo.git_ai(&["diff", "HEAD"]).unwrap();
    assert!(
        diff_output.contains("similarity index 100%") || diff_output.contains("rename from"),
        "git ai diff should recognize pure renames"
    );

    // Stats should show 0 additions and 0 deletions for pure renames
    let stats = stats_from_args(&repo, &["stats", "HEAD", "--json"]);
    assert_eq!(
        stats.git_diff_added_lines, 0,
        "Pure renames should not count as additions"
    );
    assert_eq!(
        stats.git_diff_deleted_lines, 0,
        "Pure renames should not count as deletions"
    );
    assert_eq!(stats.ai_additions, 0);
    assert_eq!(stats.human_additions, 0);
}

crate::reuse_tests_in_worktree!(
    test_stats_default_ignores_snapshot_files,
    test_stats_default_ignores_lockfiles_and_generated_files,
    test_stats_ignores_linguist_generated_patterns,
    test_stats_keeps_negative_linguist_patterns_counted,
    test_stats_in_bare_clone_uses_root_gitattributes_linguist_generated,
    test_stats_ignore_flag_is_additive_to_defaults,
    test_stats_range_uses_default_ignores,
    test_post_commit_large_ignored_files_do_not_trigger_skip_warning,
    test_stats_ignores_renamed_files,
);
