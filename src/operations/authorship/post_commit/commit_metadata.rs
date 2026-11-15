use crate::error::GitAiError;
use crate::operations::git::repository::Repository;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CommitMetricMetadata {
    pub subject: Option<String>,
    pub body: Option<String>,
    pub author_ts: Option<u64>,
    pub commit_ts: Option<u64>,
}

pub(crate) fn commit_metric_metadata(
    repo: &Repository,
    commit_sha: &str,
) -> Result<CommitMetricMetadata, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.extend([
        "show".to_string(),
        "-s".to_string(),
        "--no-notes".to_string(),
        "--encoding=UTF-8".to_string(),
        "--format=%s%x00%b%x00%at%x00%ct".to_string(),
        commit_sha.to_string(),
    ]);
    let output = crate::clients::git_cli::exec_git(&args)?;
    let stdout = String::from_utf8(output.stdout)?;
    Ok(parse_commit_metric_metadata_output(&stdout))
}

pub(super) fn parse_commit_metric_metadata_output(output: &str) -> CommitMetricMetadata {
    let mut parts = output.splitn(4, '\0');
    let Some(subject) = parts.next() else {
        return CommitMetricMetadata::default();
    };
    let Some(body) = parts.next() else {
        return CommitMetricMetadata::default();
    };
    let Some(author_ts) = parts.next() else {
        return CommitMetricMetadata::default();
    };
    let Some(commit_ts) = parts.next() else {
        return CommitMetricMetadata::default();
    };

    let subject = subject.trim().to_string();
    let body = body.trim().to_string();

    CommitMetricMetadata {
        subject: Some(subject),
        body: (!body.is_empty()).then_some(body),
        author_ts: author_ts.trim().parse::<u64>().ok(),
        commit_ts: commit_ts.trim().parse::<u64>().ok(),
    }
}
