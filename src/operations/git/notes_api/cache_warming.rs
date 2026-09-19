use super::*;
use crate::clients::git_cli::exec_git_with_stdin_writer;
use crate::operations::git::oid::is_non_zero_oid;

/// Warm recent remote-default history for clone and explicit/manual callers.
/// Fetch/pull side effects use `warm_cache_for_revisions` instead, because refs
/// may have moved by the time their asynchronous processing reaches this code.
pub fn warm_cache_for_remote(repo: &Repository, remote: &str) -> Result<(), GitAiError> {
    use crate::clients::git_cli::exec_git;

    // 1. Walk recent history. Prefer the remote's default branch; fall back to HEAD.
    let remote_head = format!("refs/remotes/{}/HEAD", remote);
    let rev_target = {
        let check_args: Vec<String> = repo
            .global_args_for_exec()
            .into_iter()
            .chain([
                "rev-parse".to_string(),
                "--verify".to_string(),
                "--quiet".to_string(),
                remote_head.clone(),
            ])
            .collect();
        if exec_git(&check_args)
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            remote_head
        } else {
            "HEAD".to_string()
        }
    };

    warm_cache_for_targets(repo, &[rev_target])
}

/// Walk only immutable incoming OIDs captured for a transport operation.
/// One stdin-fed Git process returns at most 500 commits across all tips.
pub fn warm_cache_for_revisions(repo: &Repository, revisions: &[String]) -> Result<(), GitAiError> {
    let mut seen = HashSet::new();
    let revisions: Vec<String> = revisions
        .iter()
        .filter(|oid| is_non_zero_oid(oid) && seen.insert(oid.as_str()))
        .cloned()
        .collect();
    warm_cache_for_targets(repo, &revisions)
}

fn warm_cache_for_targets(repo: &Repository, revisions: &[String]) -> Result<(), GitAiError> {
    use crate::clients::api::client::{ApiClient, ApiContext};

    if revisions.is_empty() {
        return Ok(());
    }

    let rev_list_args: Vec<String> = repo
        .global_args_for_exec()
        .into_iter()
        .chain([
            "rev-list".to_string(),
            "--max-count=500".to_string(),
            "--stdin".to_string(),
        ])
        .collect();

    let output = exec_git_with_stdin_writer(&rev_list_args, |writer| {
        for revision in revisions {
            writer.write_all(revision.as_bytes())?;
            writer.write_all(b"\n")?;
        }
        Ok(())
    })?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let all_shas: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if all_shas.is_empty() {
        tracing::debug!("warm_cache_for_remote: no commits in HEAD history; skipping");
        return Ok(());
    }

    // 2. Filter out SHAs already in notes-db.
    let already_cached: HashSet<String> = {
        match crate::model::repository::notes_db::NotesDatabase::global() {
            Ok(db) => match db.lock() {
                Ok(lock) => {
                    let refs: Vec<&str> = all_shas.iter().map(|s| s.as_str()).collect();
                    lock.get_notes(&refs)
                        .unwrap_or_default()
                        .into_keys()
                        .collect()
                }
                Err(e) => {
                    tracing::warn!("warm_cache_for_remote: DB lock poisoned: {}", e);
                    HashSet::new()
                }
            },
            Err(e) => {
                tracing::warn!("warm_cache_for_remote: failed to open notes-db: {}", e);
                HashSet::new()
            }
        }
    };

    let uncached: Vec<String> = all_shas
        .into_iter()
        .filter(|sha| !already_cached.contains(sha))
        .collect();

    if uncached.is_empty() {
        tracing::debug!("warm_cache_for_remote: all commits already cached; skipping");
        return Ok(());
    }

    tracing::info!(
        backend = %"http",
        uncached_commits = uncached.len(),
        "fetching authorship notes"
    );
    tracing::debug!(
        "warm_cache_for_remote: fetching notes for {} uncached commits",
        uncached.len()
    );

    // 3. Batch-fetch from the HTTP backend (chunks of 100).
    let cfg = crate::config::Config::fresh();
    let Some(backend_url) = cfg.notes_backend_url().map(str::to_string) else {
        tracing::debug!(
            "warm_cache_for_remote: notes_backend.backend_url is not configured; skipping"
        );
        return Ok(());
    };
    let ctx = ApiContext::new(Some(backend_url), resolve_api_author_identity);
    let client = ApiClient::new(ctx);

    // Skip when not authenticated (matches daemon flush_notes pattern).
    if !client.is_logged_in() && !client.has_api_key() {
        tracing::debug!("warm_cache_for_remote: not authenticated; skipping");
        return Ok(());
    }

    for chunk in uncached.chunks(100) {
        let sha_refs: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
        match client.read_notes(&sha_refs) {
            Ok(response) => {
                if response.notes.is_empty() {
                    continue;
                }
                // 4. Write returned entries as already-synced cache rows.
                let entries: Vec<(String, String)> = response.notes.into_iter().collect();
                match crate::model::repository::notes_db::NotesDatabase::global() {
                    Ok(db) => match db.lock() {
                        Ok(mut lock) => {
                            if let Err(e) = lock.cache_synced_notes(&entries) {
                                tracing::warn!(
                                    "warm_cache_for_remote: cache_synced_notes error: {}",
                                    e
                                );
                            } else {
                                tracing::debug!(
                                    count = entries.len(),
                                    "warm_cache_for_remote: cached notes from remote"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("warm_cache_for_remote: DB lock poisoned: {}", e);
                        }
                    },
                    Err(e) => {
                        tracing::warn!("warm_cache_for_remote: failed to open notes-db: {}", e);
                    }
                }
            }
            Err(e) => {
                // Best-effort: log and continue.
                tracing::warn!("warm_cache_for_remote: read_notes error: {}", e);
            }
        }
    }

    Ok(())
}
