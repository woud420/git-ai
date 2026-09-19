use std::collections::{HashMap, HashSet};

use crate::clients::git_cli::exec_git;
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::{AUTHORSHIP_LOG_VERSION, AuthorshipLog};
use crate::operations::authorship::conflict_resolution::{
    merge_conflict_resolution_authorship, retain_referenced_metadata,
};
use crate::operations::authorship::rewrite::{compute_diff_trees_batch, shift_authorship_log};
use crate::operations::git::{notes_api, oid::is_full_oid, repository::Repository};

const MAX_HISTORY_COMMITS: usize = 256;

struct HistoryCommit {
    oid: String,
    parent: Option<String>,
}

/// Follow every intervening change: comparing only an old note's tree with
/// the tip would resurrect attribution after deletion and identical recreation.
pub(super) fn recover_source_logs(
    repo: &Repository,
    tips: &[String],
    direct_notes: &HashMap<String, String>,
) -> Result<HashMap<String, AuthorshipLog>, GitAiError> {
    if tips.is_empty() || tips.len() > MAX_HISTORY_COMMITS {
        return Ok(HashMap::new());
    }
    let mut args = repo.global_args_for_exec();
    args.extend([
        "rev-list".to_string(),
        "--first-parent".to_string(),
        "--topo-order".to_string(),
        "--parents".to_string(),
        format!("--max-count={MAX_HISTORY_COMMITS}"),
    ]);
    args.extend(tips.iter().cloned());
    args.push("--".to_string());
    let output = exec_git(&args)?;
    let commits = parse_history(&String::from_utf8_lossy(&output.stdout));
    let Some(commits) = commits else {
        return Ok(HashMap::new());
    };
    let included: HashSet<_> = commits.iter().map(|commit| commit.oid.as_str()).collect();
    let pairs: Vec<_> = commits
        .iter()
        .filter_map(|commit| {
            let parent = commit.parent.as_ref()?;
            included
                .contains(parent.as_str())
                .then(|| (parent.clone(), commit.oid.clone()))
        })
        .collect();
    let missing: Vec<_> = commits
        .iter()
        .filter(|commit| !direct_notes.contains_key(&commit.oid))
        .map(|commit| commit.oid.clone())
        .collect();
    let mut notes = notes_api::read_notes_batch(repo, &missing)?;
    let diffs = compute_diff_trees_batch(repo, &pairs)?;
    let mut diffs: HashMap<_, _> = pairs
        .into_iter()
        .zip(diffs)
        .map(|((_, child), diff)| (child, diff))
        .collect();
    let mut consumers = HashMap::<String, usize>::new();
    for commit in &commits {
        if let Some(parent) = &commit.parent {
            *consumers.entry(parent.clone()).or_default() += 1;
        }
    }
    let requested: HashSet<_> = tips.iter().map(String::as_str).collect();
    let mut frontier = HashMap::<String, AuthorshipLog>::new();
    let mut recovered = HashMap::new();
    for commit in commits.into_iter().rev() {
        let mut inherited = commit.parent.as_ref().and_then(|parent| {
            let remaining = consumers.get_mut(parent)?;
            *remaining -= 1;
            if *remaining == 0 {
                frontier.remove(parent)
            } else {
                frontier.get(parent).cloned()
            }
        });
        if let Some(diff) = diffs.remove(&commit.oid) {
            if let Some(log) = &mut inherited {
                shift_authorship_log(log, &diff);
            }
        } else {
            // Root, merge, shallow boundary, or parent beyond the scan budget.
            inherited = None;
        }
        let raw = notes.remove(&commit.oid);
        let raw = direct_notes.get(&commit.oid).or(raw.as_ref());
        let log = match raw {
            Some(raw) => match AuthorshipLog::deserialize_from_string(raw) {
                Ok(direct) if direct.metadata.schema_version == AUTHORSHIP_LOG_VERSION => {
                    Some(merge_conflict_resolution_authorship(
                        Some(direct),
                        inherited.unwrap_or_default(),
                        &commit.oid,
                    ))
                }
                // A present but unreadable note cannot authorize older claims.
                _ => None,
            },
            None => inherited,
        };
        if let Some(mut log) = log {
            log.metadata.base_commit_sha = commit.oid.clone();
            retain_referenced_metadata(&mut log);
            if requested.contains(commit.oid.as_str()) {
                recovered.insert(commit.oid.clone(), log.clone());
            }
            if consumers.get(&commit.oid).copied().unwrap_or(0) > 0 {
                frontier.insert(commit.oid, log);
            }
        }
    }
    Ok(recovered)
}

fn parse_history(output: &str) -> Option<Vec<HistoryCommit>> {
    output
        .lines()
        .map(|line| {
            let mut fields = line.split_whitespace();
            let oid = fields.next()?;
            if !is_full_oid(oid) {
                return None;
            }
            let parents: Vec<_> = fields.collect();
            if !parents.iter().all(|parent| is_full_oid(parent)) {
                return None;
            }
            Some(HistoryCommit {
                oid: oid.to_string(),
                parent: (parents.len() == 1).then(|| parents[0].to_string()),
            })
        })
        .collect()
}
