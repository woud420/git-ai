use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::operations::git::repository::Repository;

pub(super) fn parse_output_error(what: &str) -> GitAiError {
    GitAiError::Generic(format!("Failed to parse {} output", what))
}

/// Check if a ref exists in the repository
pub fn ref_exists(repo: &Repository, ref_name: &str) -> bool {
    let mut args = repo.global_args_for_exec();
    args.push("show-ref".to_string());
    args.push("--verify".to_string());
    args.push("--quiet".to_string());
    args.push(ref_name.to_string());

    exec_git(&args).is_ok()
}
