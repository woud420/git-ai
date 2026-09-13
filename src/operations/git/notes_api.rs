//! Centralized notes I/O API.
//!
//! All authorship-note reads and writes flow through this module. The implementation
//! dispatches to the sqlite backend (default), the git-notes backend, or the HTTP
//! backend based on `Config::fresh().notes_backend_kind()`.
//!
//! Dispatch reads a fresh config snapshot (`Config::fresh()`) rather than the
//! process-lifetime `Config::get()` singleton: the daemon is long-lived and must
//! observe backend changes made after it started.
//!
//! The sqlite backend stores notes as local-primary rows in the notes database;
//! reads fall back to `refs/notes/ai` (and backfill the cache) so repositories
//! with pre-existing git notes keep working without migration.

mod primary_backend;

use crate::config::{Config, NotesBackendKind};
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::git::notes_store::{
    AuthorshipNoteStore, GitNotesStore, HttpNoteStore, SqliteNoteStore, db_check_exists,
    db_read_notes,
};
use crate::operations::git::repository::{Repository, resolve_api_author_identity};
use std::collections::{HashMap, HashSet};

// Re-export CommitAuthorship so callers don't need to import from refs directly.
pub use crate::operations::git::refs::CommitAuthorship;
pub(crate) use primary_backend::read_authorship as read_authorship_from_primary_backend;

/// Per-SHA note-write pair: `(commit_sha, serialized_note_content)`.
///
/// The canonical element type for all `write_notes_batch` / `export_notes_to_git_refs`
/// calls. Multi-commit loops collect `Vec<NoteWriteEntry>` in memory and flush once.
/// Distinct from `notes_add_blob_batch`'s `(commit_sha, blob_oid)` shape.
pub type NoteWriteEntry = (String, String);

// --- Writes ---

pub fn write_note(repo: &Repository, commit_sha: &str, content: &str) -> Result<(), GitAiError> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite => SqliteNoteStore::new().write_note(commit_sha, content),
        NotesBackendKind::Http => HttpNoteStore::new().write_note(commit_sha, content),
        NotesBackendKind::GitNotes => GitNotesStore { repo }.write_note(commit_sha, content),
    }
}

pub fn write_notes_batch(repo: &Repository, entries: &[NoteWriteEntry]) -> Result<(), GitAiError> {
    if entries.is_empty() {
        return Ok(());
    }
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite => SqliteNoteStore::new().write_notes_batch(entries),
        NotesBackendKind::Http => HttpNoteStore::new().write_notes_batch(entries),
        NotesBackendKind::GitNotes => GitNotesStore { repo }.write_notes_batch(entries),
    }
}

// --- Reads ---

pub fn read_note(repo: &Repository, commit_sha: &str) -> Option<String> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite => SqliteNoteStore::new()
            .read_note(commit_sha)
            .or_else(|| sqlite_fallback_read_from_refs(repo, commit_sha)),
        NotesBackendKind::Http => HttpNoteStore::new()
            .read_note(commit_sha)
            .or_else(|| GitNotesStore { repo }.read_note(commit_sha)),
        NotesBackendKind::GitNotes => GitNotesStore { repo }.read_note(commit_sha),
    }
}

/// Read note contents for multiple commits in O(1) git process calls.
/// Returns a map of commit_sha → note_content for commits that have notes.
///
/// On the HTTP backend this checks the local cache, then fetches-and-caches any
/// misses from the remote, and finally falls back to local git notes; on the
/// GitNotes backend it reads directly via the batched `notes_for_commits` path.
pub fn read_notes_batch(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    if commit_shas.is_empty() {
        return Ok(HashMap::new());
    }

    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite => {
            let mut notes = SqliteNoteStore::new().read_notes_batch(commit_shas)?;

            // Fall back to refs/notes/ai for misses and backfill the cache so
            // subsequent reads are served from the database. Refs read errors
            // (e.g. corrupt note blobs) propagate — callers such as rewrite
            // migration must fail closed rather than treat notes as absent.
            let missing: Vec<String> = commit_shas
                .iter()
                .filter(|sha| !notes.contains_key(*sha))
                .cloned()
                .collect();
            if !missing.is_empty() {
                let git_notes = GitNotesStore { repo }.read_notes_batch(&missing)?;
                SqliteNoteStore::backfill_cache(&git_notes);
                notes.extend(git_notes);
            }

            Ok(notes)
        }
        NotesBackendKind::Http => {
            // db + remote-fetch tier (inside HttpNoteStore::read_notes_batch)
            let mut notes = HttpNoteStore::new().read_notes_batch(commit_shas)?;

            // Final refs fallback — errors swallowed (Http arm must not fail closed;
            // contrast with Sqlite arm above where errors propagate).
            let missing: Vec<String> = commit_shas
                .iter()
                .filter(|sha| !notes.contains_key(*sha))
                .cloned()
                .collect();
            if !missing.is_empty()
                && let Ok(git_notes) = (GitNotesStore { repo }).read_notes_batch(&missing)
            {
                notes.extend(git_notes);
            }

            Ok(notes)
        }
        NotesBackendKind::GitNotes => GitNotesStore { repo }.read_notes_batch(commit_shas),
    }
}

pub fn read_authorship(repo: &Repository, commit_sha: &str) -> Option<AuthorshipLog> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite => {
            // Check the database first; fall through to git notes on miss and
            // backfill the raw content so the next read is served locally.
            SqliteNoteStore::new()
                .read_note(commit_sha)
                .or_else(|| sqlite_fallback_read_from_refs(repo, commit_sha))
                .and_then(primary_backend::deserialize_authorship)
        }
        NotesBackendKind::Http => {
            // Check the cache first; fall through to git notes on miss.
            if let Some(content) = HttpNoteStore::new().read_note(commit_sha) {
                primary_backend::deserialize_authorship(content)
            } else {
                crate::operations::git::refs::get_authorship(repo, commit_sha)
            }
        }
        NotesBackendKind::GitNotes => {
            primary_backend::read_authorship(repo, commit_sha, NotesBackendKind::GitNotes)
        }
    }
}

pub fn read_authorship_v3(
    repo: &Repository,
    commit_sha: &str,
) -> Result<AuthorshipLog, GitAiError> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite => {
            if let Some(content) = SqliteNoteStore::new()
                .read_note(commit_sha)
                .or_else(|| sqlite_fallback_read_from_refs(repo, commit_sha))
            {
                AuthorshipLog::deserialize_from_string(&content)
                    .map_err(|e| GitAiError::Generic(format!("notes deserialization error: {}", e)))
            } else {
                crate::operations::git::refs::get_reference_as_authorship_log_v3(repo, commit_sha)
            }
        }
        NotesBackendKind::Http => {
            if let Some(content) = HttpNoteStore::new().read_note(commit_sha) {
                AuthorshipLog::deserialize_from_string(&content)
                    .map_err(|e| GitAiError::Generic(format!("notes deserialization error: {}", e)))
            } else {
                crate::operations::git::refs::get_reference_as_authorship_log_v3(repo, commit_sha)
            }
        }
        NotesBackendKind::GitNotes => {
            crate::operations::git::refs::get_reference_as_authorship_log_v3(repo, commit_sha)
        }
    }
}

/// Return a map of commit SHA → note-blob OID for the given commits.
///
/// Callers use the returned OIDs as git object IDs with the batched `cat-file` reader
/// (not purely presence checks). On Http/Sqlite backends notes live in notes-db, not in
/// git refs, so an empty map is returned and callers fall back to `read_note`.
pub fn read_note_blob_oids(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    match Config::fresh().notes_backend_kind() {
        // For Sqlite/Http, notes are in notes-db not in git — no blob OIDs exist.
        // Return an empty map; callers handle this as "no notes in git".
        NotesBackendKind::Sqlite | NotesBackendKind::Http => Ok(HashMap::new()),
        NotesBackendKind::GitNotes => {
            crate::operations::git::refs::note_blob_oids_for_commits(repo, commit_shas)
        }
    }
}

pub fn commits_with_notes(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashSet<String>, GitAiError> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite | NotesBackendKind::Http => {
            // Check the database first; fall through to git notes for misses.
            let cached = db_check_exists(commit_shas);
            if cached.len() == commit_shas.len() {
                return Ok(cached);
            }
            // For commits not in the cache, check git notes as fallback.
            let missing: Vec<String> = commit_shas
                .iter()
                .filter(|sha| !cached.contains(*sha))
                .cloned()
                .collect();
            let from_git =
                crate::operations::git::refs::commits_with_authorship_notes(repo, &missing)?;
            Ok(cached.into_iter().chain(from_git).collect())
        }
        NotesBackendKind::GitNotes => (GitNotesStore { repo }).commits_with_notes(commit_shas),
    }
}

pub fn filter_commits_with_notes(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<Vec<CommitAuthorship>, GitAiError> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite | NotesBackendKind::Http => {
            // `CommitAuthorship` requires a git_author that is only available from
            // `git rev-list`. Call the underlying git function which handles author
            // lookup, then patch in cache hits for commits whose `authorship_log`
            // would otherwise be absent (because refs/notes/ai is empty).
            //
            // The git function calls `get_authorship(repo, sha)` (refs.rs, not
            // notes_api), so for Http the results will be `CommitAuthorship::NoLog`
            // for all commits. We promote any commit that has a cache entry to
            // `CommitAuthorship::Log`.
            let cached_map = db_read_notes(commit_shas);

            let git_results =
                crate::operations::git::refs::get_commits_with_notes_from_list(repo, commit_shas)?;

            // Promote NoLog entries that are in the cache to Log entries.
            let results = git_results
                .into_iter()
                .map(|ca| match ca {
                    CommitAuthorship::NoLog {
                        ref sha,
                        ref git_author,
                    } => {
                        if let Some(content) = cached_map.get(sha)
                            && let Ok(authorship_log) =
                                AuthorshipLog::deserialize_from_string(content)
                                    .map_err(|e| GitAiError::Generic(e.to_string()))
                        {
                            return CommitAuthorship::Log {
                                sha: sha.clone(),
                                git_author: git_author.clone(),
                                authorship_log,
                            };
                        }
                        ca
                    }
                    // Already has a log (shouldn't happen for Http, but keep it).
                    CommitAuthorship::Log { .. } => ca,
                })
                .collect();

            Ok(results)
        }
        NotesBackendKind::GitNotes => {
            crate::operations::git::refs::get_commits_with_notes_from_list(repo, commit_shas)
        }
    }
}

// --- Search ---

/// Search authorship-note content for a literal substring and return matching
/// commit SHAs, newest first.
///
/// On the HTTP backend this searches the notes-db cache and unions in any
/// matches from local git notes (transition-period repos may have both); on
/// the GitNotes backend it greps `refs/notes/ai` directly.
pub fn search_notes(repo: &Repository, pattern: &str) -> Result<Vec<String>, GitAiError> {
    match Config::fresh().notes_backend_kind() {
        NotesBackendKind::Sqlite | NotesBackendKind::Http => {
            let mut shas: HashSet<String> = {
                // db-side search (same for both Sqlite and Http)
                let db_results = SqliteNoteStore::new().search_notes(pattern)?;
                db_results.into_iter().collect()
            };

            // Union in matches from local git notes for transition-period repos.
            if let Ok(git_shas) = (GitNotesStore { repo }).search_notes(pattern) {
                shas.extend(git_shas);
            }

            crate::operations::git::refs::sort_commit_shas_by_date_desc(repo, shas)
        }
        NotesBackendKind::GitNotes => (GitNotesStore { repo }).search_notes(pattern),
    }
}

// --- Materialization (for git ai log) ---

/// Materialize notes from the local cache into a one-off git ref
/// `refs/notes/ai-display` so that `git log --notes=ai-display` can render
/// them without requiring them to be in `refs/notes/ai`.
///
/// Only the most recent `limit` commits reachable from HEAD are considered.
///
/// The ref is left in place after the call; callers use it with `--notes=ai-display`.
/// It is safe to call repeatedly — each call starts from an empty tree via
/// `from 0000...` so stale notes from prior calls are discarded.
///
/// Returns the number of notes that were written into `refs/notes/ai-display`.
pub fn materialize_notes_for_display(repo: &Repository, limit: usize) -> Result<usize, GitAiError> {
    use crate::clients::git_cli::exec_git;
    use crate::clients::git_cli::exec_git_with_stdin_writer;

    // 1. Get recent commits via rev-list.
    let rev_list_args: Vec<String> = repo
        .global_args_for_exec()
        .into_iter()
        .chain([
            "rev-list".to_string(),
            format!("--max-count={}", limit),
            "HEAD".to_string(),
        ])
        .collect();

    let output = exec_git(&rev_list_args)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let commit_shas: Vec<String> = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    if commit_shas.is_empty() {
        return Ok(0);
    }

    // 2. Look up which commits are in the local notes-db cache.
    let cached_map = db_read_notes(&commit_shas);
    if cached_map.is_empty() {
        return Ok(0);
    }

    // 3. Stream a git fast-import script directly to the child's stdin.
    //    Structure:
    //      - One `blob` stanza per note (each gets a mark ID).
    //      - One `commit` stanza with `from 0000...` (empty tree) that attaches all blobs.
    let fast_import_args = crate::operations::git::refs::fast_import_args(repo);

    exec_git_with_stdin_writer(&fast_import_args, |writer| {
        for (idx, (_commit_sha, content)) in cached_map.iter().enumerate() {
            crate::operations::git::refs::write_blob_stanza(writer, idx + 1, content)?;
        }

        // Use `from` with an all-zeros SHA to start from an empty tree, ensuring
        // stale notes from prior materializations are removed.
        crate::operations::git::refs::write_notes_commit_header(
            writer,
            "refs/notes/ai-display",
            format_args!("git-ai <git-ai@localhost> 1000000000 +0000"),
            Some("0000000000000000000000000000000000000000"),
        )?;

        for (idx, (commit_sha, _content)) in cached_map.iter().enumerate() {
            writeln!(writer, "M 100644 :{} {}", idx + 1, commit_sha)?;
        }
        writer.write_all(b"\n")
    })?;

    Ok(cached_map.len())
}

// --- Cache warming ---

/// Pre-warm the local notes cache during `git pull` by fetching notes for
/// recently-arrived commits from the HTTP backend.
///
/// Algorithm:
/// 1. Walk the last 500 commits reachable from HEAD via `git rev-list`.
/// 2. Filter out any SHAs already present in `notes-db` (already cached).
/// 3. Batch the remaining SHAs into chunks of 100 and call `ApiClient::read_notes()`.
/// 4. Write returned entries via `cache_synced_notes()` so rows are inserted
///    with `synced = 1` (read cache, not upload queue).
///
/// This function is a best-effort operation: errors are logged but not propagated
/// (callers should treat failure as a cache miss, not a hard error).
pub fn warm_cache_for_remote(repo: &Repository, remote: &str) -> Result<(), GitAiError> {
    use crate::clients::api::client::{ApiClient, ApiContext};
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

    let rev_list_args: Vec<String> = repo
        .global_args_for_exec()
        .into_iter()
        .chain([
            "rev-list".to_string(),
            "--max-count=500".to_string(),
            rev_target,
        ])
        .collect();

    let output = exec_git(&rev_list_args)?;
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
        remote = %remote,
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

// --- Backend bypass ---

/// Export entries directly into `refs/notes/ai` regardless of the configured
/// backend. Used by `git-ai notes migrate --to git-notes` to share sqlite-backed
/// attribution via the notes ref.
pub fn export_notes_to_git_refs(
    repo: &Repository,
    entries: &[NoteWriteEntry],
) -> Result<(), GitAiError> {
    crate::operations::git::refs::notes_add_batch(repo, entries)
}

// --- Private helpers ---

/// Sqlite backend: read a single note from refs/notes/ai on database miss and
/// backfill it into the cache so the next read is served from the database.
fn sqlite_fallback_read_from_refs(repo: &Repository, commit_sha: &str) -> Option<String> {
    let content = GitNotesStore { repo }.read_note(commit_sha)?;
    SqliteNoteStore::backfill_cache(&HashMap::from([(commit_sha.to_string(), content.clone())]));
    Some(content)
}

// --- Tests ---

#[path = "notes_api_tests.rs"]
#[cfg(test)]
mod tests;

#[path = "notes_api_warm_cache_tests.rs"]
#[cfg(test)]
mod warm_cache_tests;
