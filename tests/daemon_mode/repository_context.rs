use super::*;

#[test]
fn explicit_repository_context_preserves_attribution_when_c_directory_names_another_repo() {
    for environment_override in [false, true] {
        let repo = TestRepo::new_dedicated_daemon();
        let decoy = TestRepo::new();
        let mut decoy_file = decoy.filename("decoy.txt");
        decoy_file.set_contents(lines!["other repository".human()]);
        decoy.stage_all_and_commit("Seed other repository").unwrap();
        decoy_file.assert_committed_lines(lines!["other repository".human()]);

        let mut file = repo.filename("target.txt");
        file.set_contents(lines!["target AI".ai()]);
        repo.git(&["add", "target.txt"]).unwrap();
        let git_dir = repo.path().join(".git").to_string_lossy().into_owned();
        let worktree = repo.path().to_string_lossy().into_owned();
        let decoy_path = decoy.path().to_string_lossy().into_owned();
        if environment_override {
            repo.git_with_env(
                &[
                    "-C",
                    &decoy_path,
                    "commit",
                    "-m",
                    "Explicit environment target",
                ],
                &[("GIT_DIR", &git_dir), ("GIT_WORK_TREE", &worktree)],
                None,
            )
            .unwrap();
        } else {
            repo.git(&[
                "-C",
                &decoy_path,
                "--git-dir",
                &git_dir,
                "--work-tree",
                &worktree,
                "commit",
                "-m",
                "Explicit argument target",
            ])
            .unwrap();
        }
        file.assert_committed_lines(lines!["target AI".ai()]);
        decoy_file.assert_committed_lines(lines!["other repository".human()]);
    }
}
