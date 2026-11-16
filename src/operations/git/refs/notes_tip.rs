use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::operations::git::fast_reader::FastRefReader;
use crate::operations::git::repository::Repository;

use super::AI_AUTHORSHIP_FULL_REF;

/// The commit `refs/notes/ai` points at, or `None` when the ref is missing.
///
/// Reads the loose ref or `packed-refs` directly, like
/// `git rev-parse --verify` (which does not check that the object exists),
/// and asks git only when the files can't answer, as with reftable.
pub(super) fn notes_ref_tip(repo: &Repository) -> Result<Option<String>, GitAiError> {
    let reader = FastRefReader::new(repo.path(), repo.common_dir());
    if let Some(tip) = reader.try_resolve_ref(AI_AUTHORSHIP_FULL_REF) {
        return Ok(Some(tip));
    }

    let mut args = repo.global_args_for_exec();
    args.extend(["rev-parse", "--verify", AI_AUTHORSHIP_FULL_REF].map(String::from));
    match exec_git(&args) {
        Ok(output) => Ok(Some(String::from_utf8(output.stdout)?.trim().to_string())),
        Err(GitAiError::GitCliError {
            code: Some(128) | Some(1),
            ..
        }) => Ok(None),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::git::test_utils::TmpRepo;

    #[test]
    fn notes_ref_tip_matches_git_for_missing_loose_and_packed_refs() {
        let tmp = TmpRepo::new().unwrap();
        let git = |args: &[&str]| tmp.git_command(args).unwrap().trim().to_string();
        let repo = tmp.gitai_repo();
        let notes_ref = repo.common_dir().join(AI_AUTHORSHIP_FULL_REF);
        assert_eq!(notes_ref_tip(repo).unwrap(), None);

        let first = tmp.commit_all("first").unwrap();
        git(&["notes", "--ref=ai", "add", "-m", "note", &first]);
        let tip = git(&["rev-parse", "--verify", AI_AUTHORSHIP_FULL_REF]);
        assert!(notes_ref.is_file());
        assert_eq!(notes_ref_tip(repo).unwrap(), Some(tip.clone()));

        git(&["pack-refs", "--all"]);
        assert!(!notes_ref.exists());
        assert_eq!(notes_ref_tip(repo).unwrap(), Some(tip));
    }
}
