use super::E;
use crate::config::Config;
use crate::operations::git::repository::{
    canonicalize_repository_policy_path, load_repository_policy_context_for_paths,
};
use crate::operations::jj::capture::registration::{RetainedCapture, SampledPolicyPaths};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub(in crate::operations::jj) fn require_opt_in(config: &Config) -> Result<(), E> {
    if !config.has_allowed_repositories() {
        return Err(E::invalid("policy", "repository collection is not allowed"));
    }
    Ok(())
}

pub(in crate::operations::jj) fn check_deadline(deadline: Instant) -> Result<(), E> {
    if Instant::now() >= deadline {
        return Err(E::invalid("deadline", "cooperative deadline elapsed"));
    }
    Ok(())
}

pub(in crate::operations::jj) fn authorize(
    session: &mut RetainedCapture<'_>,
    config: &Config,
    deadline: Instant,
) -> Result<(), E> {
    check_deadline(deadline)?;
    let sampled = session.policy_paths();
    let canonical = SampledPolicyPaths {
        workspace_root: canonicalize(&sampled.workspace_root, deadline)?,
        git_dir: canonicalize(&sampled.git_dir, deadline)?,
        git_common_dir: canonicalize(&sampled.git_common_dir, deadline)?,
    };
    check_deadline(deadline)?;
    // Config includes and the cold installation lookup belong to this off-ingress
    // policy stage; capture's metadata budget cannot bound that separate I/O.
    let policy = load_repository_policy_context_for_paths(
        &canonical.workspace_root,
        &canonical.git_dir,
        &canonical.git_common_dir,
    )
    .map_err(|error| E::caused("policy", error))?;
    check_deadline(deadline)?;
    session
        .validate_policy_paths(canonical)
        .map_err(|error| E::caused("policy binding", error))?;
    if !policy.is_collection_allowed(config) {
        return Err(E::invalid("policy", "repository collection is not allowed"));
    }
    Ok(())
}

fn canonicalize(path: &Path, deadline: Instant) -> Result<PathBuf, E> {
    check_deadline(deadline)?;
    let path =
        canonicalize_repository_policy_path(path).map_err(|error| E::caused("policy", error))?;
    check_deadline(deadline)?;
    Ok(path)
}
