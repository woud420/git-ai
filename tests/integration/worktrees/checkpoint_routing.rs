use super::{
    CheckpointKind, ExpectedLineExt, GitAiRepository, canonicalize_for_assert,
    expected_worktree_storage_prefix, fs, raw_git, simulate_claude_post_tool_use,
    simulate_claude_pre_tool_use, unique_worktree_path,
};

#[test]
fn checkpoint_routes_to_linked_worktree_when_cwd_is_main_repo() {
    // Setup: main repo with an initial commit so we have a HEAD SHA.
    let repo = crate::repos::test_repo::TestRepo::new();
    let mut seed = repo.filename("seed.txt");
    seed.set_contents(crate::lines!["seed".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    // Resolve the main repo's root (the TestRepo is already a linked worktree
    // in worktree_test_wrappers!, but here we use the plain TestRepo whose CWD
    // is the repo root itself).
    let main_repo_root = repo.path().to_path_buf();

    // Create a second linked worktree alongside the main working tree.
    let linked_wt = unique_worktree_path();
    raw_git(
        &main_repo_root,
        &["worktree", "add", linked_wt.to_str().unwrap()],
    );

    // Write a file inside the linked worktree.
    let wt_file = linked_wt.join("feature.rs");
    fs::write(
        &wt_file,
        "fn feature() {}\nfn feature2() {}\nfn feature3() {}\n",
    )
    .expect("write wt file");

    // Simulate PostToolUse hook: CWD = main repo, file = linked worktree.
    // This is the exact scenario that caused BUG-A.
    simulate_claude_post_tool_use(&repo, &main_repo_root, &wt_file)
        .expect("checkpoint should succeed");

    // The checkpoint must land in the *linked worktree's* isolated storage,
    // not in the main repo's plain working_logs.
    let wt_repo = GitAiRepository::find_repository_in_path(linked_wt.to_str().unwrap())
        .expect("find linked worktree repo");

    // Storage must be rooted under the repository's shared git-common-dir.
    // We ask git for that path directly instead of canonicalizing the repo
    // root, because Windows canonicalize() uses extended-length path syntax.
    let expected_storage_prefix =
        canonicalize_for_assert(&expected_worktree_storage_prefix(&main_repo_root));
    let actual_storage = canonicalize_for_assert(&wt_repo.storage.working_logs);
    assert!(
        actual_storage.starts_with(&expected_storage_prefix),
        "linked worktree storage should be under git-common-dir/ai/worktrees:\nexpected prefix: {}\nactual: {}",
        expected_storage_prefix.display(),
        actual_storage.display()
    );

    let commit_sha = wt_repo
        .head()
        .expect("wt repo should have a HEAD")
        .target()
        .expect("HEAD should resolve");
    let wt_working_log = wt_repo
        .storage
        .working_log_for_base_commit(&commit_sha)
        .expect("open worktree working log");

    let checkpoints = wt_working_log
        .read_all_checkpoints()
        .expect("read checkpoints");

    assert!(
        !checkpoints.is_empty(),
        "expected at least one checkpoint in the linked worktree's working log"
    );

    let ai_checkpoint = checkpoints
        .iter()
        .find(|cp| cp.kind == CheckpointKind::AiAgent)
        .expect("expected an AiAgent checkpoint for the Write");

    assert_eq!(
        ai_checkpoint.entries.len(),
        1,
        "checkpoint should cover exactly the written file"
    );
    assert_eq!(
        ai_checkpoint.entries[0].file, "feature.rs",
        "checkpoint entry should use the worktree-relative path"
    );

    // Cleanup the temporary worktree.
    raw_git(
        &main_repo_root,
        &["worktree", "remove", "--force", linked_wt.to_str().unwrap()],
    );
}

/// Same as `checkpoint_routes_to_linked_worktree_when_cwd_is_main_repo` but the linked
/// worktree lives *inside* the main repo's working tree (e.g. `.worktrees/feature`).
///
/// This is the exact structure that caused Bug-A / Bug-B: `path_is_in_workdir` returned
/// `true` because the file's path starts with the main repo's workdir, and only the
/// `.git` FILE detection (is_linked_worktree_git_file) distinguishes it from a regular
/// subdirectory.  Without the fix this test fails silently — the checkpoint is skipped
/// and `checkpoints.jsonl` remains empty.
#[test]
fn checkpoint_routes_to_nested_linked_worktree_when_cwd_is_main_repo() {
    let repo = crate::repos::test_repo::TestRepo::new();
    let mut seed = repo.filename("seed.txt");
    seed.set_contents(crate::lines!["seed".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    let main_repo_root = repo.path().to_path_buf();

    // Worktree lives INSIDE the main repo's working tree.
    let linked_wt = main_repo_root.join(".worktrees").join("feature");
    raw_git(
        &main_repo_root,
        &["worktree", "add", "--detach", linked_wt.to_str().unwrap()],
    );

    // Sanity: the .git file inside the nested worktree must point to /worktrees/
    let dot_git_content =
        fs::read_to_string(linked_wt.join(".git")).expect("read nested worktree .git file");
    assert!(
        dot_git_content.contains("/worktrees/"),
        "nested worktree .git file should contain /worktrees/: {}",
        dot_git_content.trim()
    );

    let wt_file = linked_wt.join("feature.rs");
    fs::write(
        &wt_file,
        "fn feature() {}\nfn feature2() {}\nfn feature3() {}\n",
    )
    .expect("write wt file");

    // PostToolUse: CWD = main repo, file = *nested* linked worktree.
    // Before the fix, path_is_in_workdir wrongly returned true here
    // (the file path starts_with the main workdir), so git status found
    // nothing and the checkpoint was silently dropped.
    simulate_claude_post_tool_use(&repo, &main_repo_root, &wt_file)
        .expect("checkpoint should succeed");

    let wt_repo = GitAiRepository::find_repository_in_path(linked_wt.to_str().unwrap())
        .expect("find nested worktree repo");

    // Storage must be rooted under the repository's shared git-common-dir.
    // We ask git for that path directly instead of canonicalizing the repo
    // root, because Windows canonicalize() uses extended-length path syntax.
    let expected_storage_prefix =
        canonicalize_for_assert(&expected_worktree_storage_prefix(&main_repo_root));
    let actual_storage = canonicalize_for_assert(&wt_repo.storage.working_logs);
    assert!(
        actual_storage.starts_with(&expected_storage_prefix),
        "nested worktree storage should be under git-common-dir/ai/worktrees:\nexpected prefix: {}\nactual: {}",
        expected_storage_prefix.display(),
        actual_storage.display()
    );

    let commit_sha = wt_repo
        .head()
        .expect("wt repo should have a HEAD")
        .target()
        .expect("HEAD should resolve");
    let wt_working_log = wt_repo
        .storage
        .working_log_for_base_commit(&commit_sha)
        .expect("open worktree working log");

    let checkpoints = wt_working_log
        .read_all_checkpoints()
        .expect("read checkpoints");

    assert!(
        !checkpoints.is_empty(),
        "expected a checkpoint in the nested worktree's working log — \
         if this fails the .git FILE boundary detection is broken"
    );

    let ai_checkpoint = checkpoints
        .iter()
        .find(|cp| cp.kind == CheckpointKind::AiAgent)
        .expect("expected an AiAgent checkpoint for the Write");

    assert_eq!(
        ai_checkpoint.entries.len(),
        1,
        "checkpoint should cover exactly the written file"
    );
    assert_eq!(
        ai_checkpoint.entries[0].file, "feature.rs",
        "checkpoint entry should use the worktree-relative path"
    );

    raw_git(
        &main_repo_root,
        &["worktree", "remove", "--force", linked_wt.to_str().unwrap()],
    );
}

#[test]
fn human_checkpoint_routes_to_linked_worktree_when_cwd_is_main_repo() {
    // Same scenario as above, but for the PreToolUse (Human) hook.
    let repo = crate::repos::test_repo::TestRepo::new();
    let mut seed = repo.filename("seed.txt");
    seed.set_contents(crate::lines!["seed".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    let main_repo_root = repo.path().to_path_buf();

    let linked_wt = unique_worktree_path();
    raw_git(
        &main_repo_root,
        &["worktree", "add", linked_wt.to_str().unwrap()],
    );

    // Write a file in the worktree before the hook so there's an "uncaptured zone".
    let wt_file = linked_wt.join("preexisting.rs");
    fs::write(&wt_file, "fn old() {}\n").expect("write wt file");

    // PreToolUse: informs git-ai that this file is *about to* be edited.
    simulate_claude_pre_tool_use(&repo, &main_repo_root, &wt_file)
        .expect("pre-tool-use checkpoint should succeed");

    // The PreToolUse (Human) checkpoint should land in the worktree log.
    let wt_repo = GitAiRepository::find_repository_in_path(linked_wt.to_str().unwrap())
        .expect("find linked worktree repo");

    let commit_sha = wt_repo
        .head()
        .expect("wt repo should have a HEAD")
        .target()
        .expect("HEAD should resolve");
    let wt_working_log = wt_repo
        .storage
        .working_log_for_base_commit(&commit_sha)
        .expect("open worktree working log");

    let checkpoints = wt_working_log
        .read_all_checkpoints()
        .expect("read checkpoints");

    // A Human checkpoint is only stored when there is an uncaptured zone
    // (i.e., content not yet attributed).  With the pre-existing file content
    // and no prior AI checkpoint, a Human entry should be recorded.
    let has_human = checkpoints
        .iter()
        .any(|cp| cp.kind == CheckpointKind::Human);
    assert!(
        has_human,
        "expected a Human checkpoint in the linked worktree's working log"
    );

    raw_git(
        &main_repo_root,
        &["worktree", "remove", "--force", linked_wt.to_str().unwrap()],
    );
}

/// Same as `human_checkpoint_routes_to_linked_worktree_when_cwd_is_main_repo` but
/// the worktree lives *inside* the main repo's working tree.  Without the
/// is_linked_worktree_git_file fix, path_is_in_workdir returns true, the Human
/// checkpoint is processed against the wrong repo, and this test fails.
#[test]
fn human_checkpoint_routes_to_nested_linked_worktree_when_cwd_is_main_repo() {
    let repo = crate::repos::test_repo::TestRepo::new();
    let mut seed = repo.filename("seed.txt");
    seed.set_contents(crate::lines!["seed".human()]);
    repo.stage_all_and_commit("initial").unwrap();

    let main_repo_root = repo.path().to_path_buf();

    // Worktree is INSIDE the main repo's working tree.
    let linked_wt = main_repo_root.join(".worktrees").join("pre-feature");
    raw_git(
        &main_repo_root,
        &["worktree", "add", "--detach", linked_wt.to_str().unwrap()],
    );

    let wt_file = linked_wt.join("preexisting.rs");
    fs::write(&wt_file, "fn old() {}\n").expect("write wt file");

    simulate_claude_pre_tool_use(&repo, &main_repo_root, &wt_file)
        .expect("pre-tool-use checkpoint should succeed");

    let wt_repo = GitAiRepository::find_repository_in_path(linked_wt.to_str().unwrap())
        .expect("find nested worktree repo");

    let commit_sha = wt_repo
        .head()
        .expect("wt repo should have a HEAD")
        .target()
        .expect("HEAD should resolve");
    let wt_working_log = wt_repo
        .storage
        .working_log_for_base_commit(&commit_sha)
        .expect("open worktree working log");

    let checkpoints = wt_working_log
        .read_all_checkpoints()
        .expect("read checkpoints");

    let has_human = checkpoints
        .iter()
        .any(|cp| cp.kind == CheckpointKind::Human);
    assert!(
        has_human,
        "expected a Human checkpoint in the nested worktree's working log — \
         if this fails the .git FILE boundary detection is broken for PreToolUse"
    );

    raw_git(
        &main_repo_root,
        &["worktree", "remove", "--force", linked_wt.to_str().unwrap()],
    );
}
