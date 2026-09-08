use super::{
    BTreeMap, BTreeSet, ExpectedLineExt, HashMap, HumanRecord, LineAttribution, SessionRecord,
    TestRepo, current_checkpoint_files, fs, single_stash_v2_initial, test_agent, test_prompt,
};

#[test]
fn test_stash_push_with_pathspec_single_file() {
    // Test git stash push -- file.txt only stashes that file
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create two files with AI content
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["file1 line 1".ai(), "file1 line 2".ai()]);

    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["file2 line 1".ai(), "file2 line 2".ai()]);

    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash only file1.txt
    repo.git(&["stash", "push", "--", "file1.txt"])
        .expect("stash push should succeed");

    // file1 should be gone, file2 should still exist
    assert!(repo.read_file("file1.txt").is_none());
    assert!(repo.read_file("file2.txt").is_some());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Now file1 is back
    assert!(repo.read_file("file1.txt").is_some());

    // Commit everything
    let commit = repo
        .stage_all_and_commit("apply partial stash")
        .expect("commit should succeed");

    // Both files should have AI attribution
    file1.assert_lines_and_blame(vec!["file1 line 1".ai(), "file1 line 2".ai()]);
    file2.assert_lines_and_blame(vec!["file2 line 1".ai(), "file2 line 2".ai()]);

    // Should have AI prompts
    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_push_with_pathspec_directory() {
    // Test git stash push -- dir/ only stashes that directory
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create files in a directory and root
    let mut root_file = repo.filename("root.txt");
    root_file.set_contents(vec!["root line 1".ai()]);

    // Create src directory
    std::fs::create_dir_all(repo.path().join("src")).expect("Failed to create src dir");

    let mut dir_file1 = repo.filename("src/file1.txt");
    dir_file1.set_contents(vec!["src file1 line 1".ai()]);

    let mut dir_file2 = repo.filename("src/file2.txt");
    dir_file2.set_contents(vec!["src file2 line 1".ai()]);

    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash only src/ directory
    repo.git(&["stash", "push", "--", "src/"])
        .expect("stash push should succeed");

    // src files should be gone, root file should remain
    assert!(repo.read_file("src/file1.txt").is_none());
    assert!(repo.read_file("src/file2.txt").is_none());
    assert!(repo.read_file("root.txt").is_some());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit everything
    let commit = repo
        .stage_all_and_commit("apply directory stash")
        .expect("commit should succeed");

    // All files should have AI attribution
    root_file.assert_lines_and_blame(vec!["root line 1".ai()]);
    dir_file1.assert_lines_and_blame(vec!["src file1 line 1".ai()]);
    dir_file2.assert_lines_and_blame(vec!["src file2 line 1".ai()]);

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_stash_push_multiple_pathspecs() {
    // Test git stash push -- file1.txt file2.txt
    let repo = TestRepo::new();

    // Create initial commit
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit")
        .expect("commit should succeed");

    // Create three files with AI content
    let mut file1 = repo.filename("file1.txt");
    file1.set_contents(vec!["file1".ai()]);

    let mut file2 = repo.filename("file2.txt");
    file2.set_contents(vec!["file2".ai()]);

    let mut file3 = repo.filename("file3.txt");
    file3.set_contents(vec!["file3".ai()]);

    repo.git_ai(&["checkpoint", "mock_ai"])
        .expect("checkpoint should succeed");

    // Stash only file1 and file2
    repo.git(&["stash", "push", "--", "file1.txt", "file2.txt"])
        .expect("stash push should succeed");

    // file1 and file2 should be gone, file3 remains
    assert!(repo.read_file("file1.txt").is_none());
    assert!(repo.read_file("file2.txt").is_none());
    assert!(repo.read_file("file3.txt").is_some());

    // Pop the stash
    repo.git(&["stash", "pop"])
        .expect("stash pop should succeed");

    // Commit everything
    let commit = repo
        .stage_all_and_commit("apply multi-pathspec stash")
        .expect("commit should succeed");

    // All files should have AI attribution
    file1.assert_lines_and_blame(vec!["file1".ai()]);
    file2.assert_lines_and_blame(vec!["file2".ai()]);
    file3.assert_lines_and_blame(vec!["file3".ai()]);

    assert!(
        !commit.authorship_log.metadata.sessions.is_empty(),
        "Expected sessions in authorship log"
    );
}

#[test]
fn test_partial_stash_truncates_oversized_live_checkpoints_before_filtering() {
    let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_CHECKPOINTS_JSONL_MAX_BYTES", "64")]);
    fs::write(repo.path().join("a.txt"), "base a\n").unwrap();
    fs::write(repo.path().join("b.txt"), "base b\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    fs::write(repo.path().join("a.txt"), "base a\nai a\n").unwrap();
    fs::write(repo.path().join("b.txt"), "base b\nai b\n").unwrap();
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    let working_log = repo.current_working_logs();
    let checkpoints_file = working_log.dir.join("checkpoints.jsonl");
    let checkpoint_line = fs::read_to_string(&checkpoints_file)
        .expect("checkpoint file exists")
        .lines()
        .find(|line| !line.trim().is_empty())
        .expect("checkpoint fixture should contain one line")
        .to_string();
    fs::write(
        &checkpoints_file,
        (0..8)
            .map(|_| checkpoint_line.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .expect("inflate checkpoint file above test limit");
    assert!(
        fs::metadata(&checkpoints_file).unwrap().len() > 64,
        "test setup should exceed the daemon's test checkpoint size limit"
    );

    repo.git(&["stash", "push", "--", "a.txt"])
        .expect("partial stash should survive oversized checkpoints.jsonl");
    repo.sync_daemon_force();

    let reset_size = fs::metadata(&checkpoints_file)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    assert_eq!(
        reset_size, 0,
        "oversized live checkpoints file should be reset before path filtering"
    );
    assert!(
        repo.current_working_logs()
            .read_all_checkpoints()
            .expect("read reset checkpoints")
            .is_empty(),
        "oversized checkpoint history should be discarded"
    );

    repo.git(&["stash", "pop"])
        .expect("stash pop after oversized checkpoint recovery should succeed");
    repo.stage_all_and_commit("commit recovered stash").unwrap();

    let mut a = repo.filename("a.txt");
    a.assert_committed_lines(crate::lines![
        "base a".unattributed_human(),
        "ai a".unattributed_human(),
    ]);
    let mut b = repo.filename("b.txt");
    b.assert_committed_lines(crate::lines![
        "base b".unattributed_human(),
        "ai b".unattributed_human(),
    ]);
}

#[test]
fn test_partial_stash_trims_unstashed_initial_metadata() {
    let repo = TestRepo::new();
    fs::write(repo.path().join("a.txt"), "base a\n").unwrap();
    fs::write(repo.path().join("b.txt"), "base b\n").unwrap();
    repo.stage_all_and_commit("initial").unwrap();

    let a_content = "a ai\n";
    let b_content = "b prompt\nb human\nb session\n";
    fs::write(repo.path().join("a.txt"), a_content).unwrap();
    fs::write(repo.path().join("b.txt"), b_content).unwrap();

    let mut files = HashMap::new();
    files.insert(
        "a.txt".to_string(),
        vec![LineAttribution::new(1, 1, "prompt_a".to_string(), None)],
    );
    files.insert(
        "b.txt".to_string(),
        vec![
            LineAttribution::new(1, 1, "prompt_b".to_string(), None),
            LineAttribution::new(2, 2, "h_b".to_string(), None),
            LineAttribution::new(3, 3, "s_b::t_1".to_string(), None),
        ],
    );

    let mut prompts = HashMap::new();
    prompts.insert("prompt_a".to_string(), test_prompt("prompt-a"));
    prompts.insert("prompt_b".to_string(), test_prompt("prompt-b"));

    let mut humans = BTreeMap::new();
    humans.insert(
        "h_b".to_string(),
        HumanRecord {
            author: "B Human <b@example.com>".to_string(),
        },
    );

    let mut file_contents = HashMap::new();
    file_contents.insert("a.txt".to_string(), a_content.to_string());
    file_contents.insert("b.txt".to_string(), b_content.to_string());

    let mut sessions = BTreeMap::new();
    sessions.insert(
        "s_b".to_string(),
        SessionRecord {
            agent_id: test_agent("session-b"),
            human_author: None,
            custom_attributes: None,
        },
    );

    repo.current_working_logs()
        .write_initial_attributions_with_contents(files, prompts, humans, file_contents, sessions)
        .unwrap();

    repo.git(&["stash", "push", "--", "a.txt"]).unwrap();
    repo.sync_daemon_force();

    let stash_initial = single_stash_v2_initial(&repo);
    assert_eq!(
        stash_initial.files.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from(["a.txt".to_string()])
    );
    assert!(
        stash_initial.prompts.contains_key("prompt_a"),
        "stashed file prompt metadata should be retained"
    );
    assert!(
        !stash_initial.prompts.contains_key("prompt_b"),
        "unstashed file prompt metadata should be dropped"
    );
    assert!(
        stash_initial.humans.is_empty(),
        "unstashed known-human metadata should be dropped"
    );
    assert!(
        stash_initial.sessions.is_empty(),
        "unstashed session metadata should be dropped"
    );

    let live_initial = repo.current_working_logs().read_initial_attributions();
    assert!(
        !live_initial.prompts.contains_key("prompt_a"),
        "live INITIAL should not retain metadata for the stashed file"
    );
    assert!(
        live_initial.prompts.contains_key("prompt_b"),
        "live INITIAL should retain metadata for the unstashed file"
    );
    assert!(
        live_initial.humans.contains_key("h_b"),
        "live INITIAL should retain unstashed known-human metadata"
    );
    assert!(
        live_initial.sessions.contains_key("s_b"),
        "live INITIAL should retain unstashed session metadata"
    );
}

/// Regression (#5): `git stash push -- <pathspec>` must only save attribution
/// for the stashed paths and leave unstashed attribution live.
#[test]
fn test_stash_push_pathspec_excludes_unstashed_file_from_stash_log() {
    let repo = TestRepo::new();
    let mut readme = repo.filename("README.md");
    readme.set_contents(vec!["# Test Repo".to_string()]);
    repo.stage_all_and_commit("initial commit").unwrap();

    let mut a = repo.filename("a.txt");
    a.set_contents(vec!["a line 1".ai(), "a line 2".ai()]);
    let mut b = repo.filename("b.txt");
    b.set_contents(vec!["b line 1".ai(), "b line 2".ai()]);
    repo.git_ai(&["checkpoint", "mock_ai"]).unwrap();

    repo.git(&["stash", "push", "--", "a.txt"]).unwrap();
    repo.sync_daemon_force();

    let stashed_files: BTreeSet<_> = single_stash_v2_initial(&repo).files.into_keys().collect();
    let live_checkpoint_files = current_checkpoint_files(&repo);

    assert!(
        stashed_files.contains("a.txt"),
        "stash should carry the stashed file a.txt, got {:?}",
        stashed_files
    );
    assert!(
        !stashed_files.contains("b.txt"),
        "stash must NOT carry the unstashed file b.txt, got {:?}",
        stashed_files
    );
    assert!(
        !live_checkpoint_files.contains("a.txt"),
        "live checkpoints must not retain stashed file a.txt, got {:?}",
        live_checkpoint_files
    );
    assert!(
        live_checkpoint_files.contains("b.txt"),
        "live checkpoints must retain unstashed file b.txt, got {:?}",
        live_checkpoint_files
    );

    repo.git(&["stash", "pop"]).unwrap();
    repo.stage_all_and_commit("apply partial stash").unwrap();
    a.assert_committed_lines(vec!["a line 1".ai(), "a line 2".ai()]);
    b.assert_committed_lines(vec!["b line 1".ai(), "b line 2".ai()]);
}

crate::reuse_tests_in_worktree!(
    test_stash_push_with_pathspec_single_file,
    test_stash_push_with_pathspec_directory,
    test_stash_push_multiple_pathspecs,
);
