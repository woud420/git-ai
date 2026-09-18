use super::*;

// range-diff retains patches and pairwise matching costs for its input commits.
const MAX_RANGE_DIFF_COMMITS: usize = 1000;

pub(super) fn bounded_new_base(
    repo: &Repository,
    old_base: &str,
    old_tip: &str,
    new_base: &str,
    new_tip: &str,
) -> Option<String> {
    let limit = commit_limit();
    let bounded = count_up_to(repo, old_base, old_tip, limit)
        .filter(|count| *count <= limit)
        .and_then(|_| bounded_tip_base(repo, new_base, new_tip, limit));
    if bounded.is_none() {
        tracing::warn!(
            old_tip,
            new_tip,
            limit,
            "skipping range-diff mapping derivation because its inputs cannot be bounded"
        );
    }
    bounded
}

fn commit_limit() -> usize {
    #[cfg(feature = "test-support")]
    if let Ok(raw) = std::env::var("GIT_AI_TEST_RANGE_DIFF_COMMIT_LIMIT")
        && let Ok(limit) = raw.parse::<usize>()
        && limit > 0
    {
        return limit.min(MAX_RANGE_DIFF_COMMITS);
    }
    MAX_RANGE_DIFF_COMMITS
}

fn count_up_to(repo: &Repository, base: &str, tip: &str, limit: usize) -> Option<usize> {
    let output = bounded_rev_list(repo, base, tip, limit, "--count")?;
    output.trim().parse().ok()
}

fn bounded_tip_base(repo: &Repository, base: &str, tip: &str, limit: usize) -> Option<String> {
    let output = bounded_rev_list(repo, base, tip, limit, "--topo-order")?;
    let Some(candidate) = output.lines().nth(limit) else {
        return Some(base.to_string());
    };
    // A topological suffix may still include a long parallel branch after
    // subtracting its boundary commit. Verify the actual range before using it.
    count_up_to(repo, candidate, tip, limit)
        .filter(|count| *count <= limit)
        .map(|_| candidate.to_string())
}

fn bounded_rev_list(
    repo: &Repository,
    base: &str,
    tip: &str,
    limit: usize,
    order_or_count: &str,
) -> Option<String> {
    let mut args = repo.global_args_for_exec();
    args.extend([
        "rev-list".to_string(),
        order_or_count.to_string(),
        format!("--max-count={}", limit + 1),
        format!("{base}..{tip}"),
    ]);
    let output = exec_git_allow_nonzero(&args).ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}
