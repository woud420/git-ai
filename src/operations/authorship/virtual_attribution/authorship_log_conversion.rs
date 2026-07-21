use super::types::{AuthorshipLogDiffContext, VirtualAttributions};
use crate::error::GitAiError;
use std::collections::HashMap;

impl VirtualAttributions {
    /// Split VirtualAttributions into committed and uncommitted buckets
    ///
    /// This method uses git diff to determine which line attributions belong in:
    /// - Bucket 1 (committed): Lines added in this commit → AuthorshipLog
    /// - Bucket 2 (uncommitted): Lines NOT added in this commit → InitialAttributions
    pub fn to_authorship_log_and_initial_working_log(
        &self,
        repo: &crate::operations::git::repository::Repository,
        parent_sha: &str,
        commit_sha: &str,
        pathspecs: Option<&std::collections::HashSet<String>>,
        final_state_snapshot: Option<&HashMap<String, String>>,
    ) -> Result<
        (
            crate::model::authorship_log_serialization::AuthorshipLog,
            crate::operations::git::repo_storage::InitialAttributions,
            HashMap<String, String>,
        ),
        GitAiError,
    > {
        self.to_authorship_log_and_initial_working_log_with_precomputed_diff(
            repo,
            parent_sha,
            commit_sha,
            pathspecs,
            final_state_snapshot,
            AuthorshipLogDiffContext::default(),
        )
    }
}
