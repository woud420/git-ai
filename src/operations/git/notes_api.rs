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

mod authorship;
mod cache_warming;
pub use cache_warming::{warm_cache_for_remote, warm_cache_for_revisions};
mod primary_backend;
pub use authorship::{read_authorship_v3, read_authorship_v3_batch, read_local_notes_batch};

use crate::config::{Config, NotesBackendKind};
use crate::error::GitAiError;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::git::notes_store::{
    AuthorshipNoteStore, GitNotesStore, HttpNoteStore, SqliteNoteStore, db_check_exists,
    db_read_notes,
};
use crate::operations::git::repository::Repository;
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

#[cfg(test)]
#[path = "notes_api_tests.rs"]
mod tests;
