//! Commit-graph ancestry queries for [`Repository`].

use super::core::Repository;
use crate::clients::git_cli::exec_git_allow_nonzero;
use crate::error::GitAiError;

impl Repository {
    /// Return whether `ancestor` is reachable from `descendant`.
    ///
    /// Revision expressions are passed directly to Git. Exit status 1 means
    /// "not an ancestor"; other failures retain their structured Git error.
    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, GitAiError> {
        let mut args = self.global_args_for_exec();
        args.extend([
            "merge-base".to_string(),
            "--is-ancestor".to_string(),
            ancestor.to_string(),
            descendant.to_string(),
        ]);

        let output = exec_git_allow_nonzero(&args)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            code => Err(GitAiError::GitCliError {
                code,
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                args,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::GitAiError;
    use crate::operations::git::repository::CommitRange;
    use crate::operations::git::test_utils::TmpRepo;

    fn linear_history() -> (TmpRepo, String, String) {
        let repo = TmpRepo::new().unwrap();
        repo.write_file("history.txt", "first\n", false).unwrap();
        let first = repo.commit_all("first").unwrap();
        repo.write_file("history.txt", "second\n", false).unwrap();
        let second = repo.commit_all("second").unwrap();
        (repo, first, second)
    }

    fn merged_history() -> (TmpRepo, String, String, String, String) {
        let repo = TmpRepo::new().unwrap();
        repo.write_file("base.txt", "base\n", false).unwrap();
        let base = repo.commit_all("base").unwrap();

        repo.git_command(&["switch", "-c", "feature"]).unwrap();
        repo.write_file("feature.txt", "feature\n", false).unwrap();
        let feature_tip = repo.commit_all("feature").unwrap();

        repo.switch_branch("main").unwrap();
        repo.write_file("main.txt", "main\n", false).unwrap();
        let main_tip = repo.commit_all("main").unwrap();
        repo.git_command(&["merge", "--no-ff", "feature", "-m", "merge"])
            .unwrap();
        let merge = repo.git_command(&["rev-parse", "HEAD"]).unwrap();

        (repo, base, main_tip, feature_tip, merge.trim().to_string())
    }

    #[test]
    fn reports_linear_and_symbolic_ancestry() {
        let (repo, first, second) = linear_history();

        assert!(repo.gitai_repo().is_ancestor(&first, &second).unwrap());
        assert!(!repo.gitai_repo().is_ancestor(&second, &first).unwrap());
        assert!(repo.gitai_repo().is_ancestor(&first, "HEAD").unwrap());
        assert!(repo.gitai_repo().is_ancestor("HEAD~1", "main").unwrap());
    }

    #[test]
    fn returns_false_for_unrelated_histories() {
        let (repo, first, _) = linear_history();
        repo.git_command(&["switch", "--orphan", "unrelated"])
            .unwrap();
        repo.write_file("unrelated.txt", "unrelated\n", false)
            .unwrap();
        let unrelated = repo.commit_all("unrelated").unwrap();

        assert!(!repo.gitai_repo().is_ancestor(&first, &unrelated).unwrap());
    }

    #[test]
    fn preserves_structured_error_for_invalid_revision() {
        let (repo, first, _) = linear_history();
        let invalid = "not-a-revision";

        let error = repo.gitai_repo().is_ancestor(&first, invalid).unwrap_err();
        let GitAiError::GitCliError { code, stderr, args } = error else {
            panic!("expected structured Git CLI error");
        };

        assert!(!matches!(code, Some(0 | 1)));
        assert!(!stderr.is_empty());
        let mut expected_args = repo.gitai_repo().global_args_for_exec();
        expected_args.extend([
            "merge-base".to_string(),
            "--is-ancestor".to_string(),
            first,
            invalid.to_string(),
        ]);
        assert_eq!(args, expected_args);
    }

    #[test]
    fn commit_range_preserves_reachability_errors() {
        let (repo, base, _, _, merge) = merged_history();
        repo.git_command(&["switch", "--orphan", "unrelated"])
            .unwrap();
        repo.write_file("unrelated.txt", "unrelated\n", false)
            .unwrap();
        let unrelated = repo.commit_all("unrelated").unwrap();
        repo.switch_branch("main").unwrap();

        let start_error = CommitRange::new_infer_refname(
            repo.gitai_repo(),
            unrelated.clone(),
            merge,
            Some("main".to_string()),
        )
        .unwrap()
        .is_valid()
        .unwrap_err();
        assert_eq!(
            start_error.to_string(),
            format!("Generic error: Commit {unrelated} is not reachable from refname main")
        );

        let end_error = CommitRange::new_infer_refname(
            repo.gitai_repo(),
            base.clone(),
            unrelated.clone(),
            Some("main".to_string()),
        )
        .unwrap()
        .is_valid()
        .unwrap_err();
        assert_eq!(
            end_error.to_string(),
            format!("Generic error: Commit {unrelated} is not reachable from refname main")
        );
    }

    #[test]
    fn commit_range_preserves_non_ancestor_error_for_reachable_siblings() {
        let (repo, _, main_tip, feature_tip, _) = merged_history();

        let error = CommitRange::new_infer_refname(
            repo.gitai_repo(),
            feature_tip.clone(),
            main_tip.clone(),
            Some("main".to_string()),
        )
        .unwrap()
        .is_valid()
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Generic error: Commit {feature_tip} is not an ancestor of {main_tip}")
        );
    }

    #[test]
    fn parent_on_refname_uses_later_matching_parent_and_preserves_exhaustion_error() {
        let (repo, _, _, feature_tip, merge) = merged_history();
        let commit = repo.gitai_repo().find_commit(merge.clone()).unwrap();

        assert_eq!(
            commit.parent_on_refname("feature").unwrap().id(),
            feature_tip
        );

        let error = match commit.parent_on_refname("missing") {
            Ok(_) => panic!("missing ref should not match a parent"),
            Err(error) => error,
        };
        assert_eq!(
            error.to_string(),
            format!("Generic error: No parent of commit {merge} is reachable from refname missing")
        );
    }
}
