//! `AuthorshipNoteStore` — per-backend storage primitives for authorship notes.
//!
//! This module defines the trait and its three implementations. Callers must go
//! through `notes_api`, which constructs the right impl per call after reading a
//! fresh config snapshot; no impl should be instantiated directly outside this
//! module or `notes_api`.
//!
//! # Fallback composition
//!
//! Cross-backend fallback flows (e.g. sqlite db-miss → refs backfill, http batch-miss
//! → remote fetch → refs swallow) are expressed explicitly in `notes_api` free fns,
//! NOT inside these impls. Impls are single-backend; composites live one layer up.
//!
//! # Asymmetries that must not be normalized
//!
//! - `SqliteNoteStore::backfill_cache` writes on refs hit; `HttpNoteStore` has no
//!   backfill (callers decide whether to backfill after a refs read).
//! - `read_notes_batch` error semantics differ across backends: see `notes_api`.
//! - `HttpNoteStore` calls `telemetry_handle::submit_notes()` after every write;
//!   this is an intentional store→daemon side-effect documented in persistence-model.md.

use crate::error::GitAiError;
use crate::operations::git::repository::Repository;
use std::collections::{HashMap, HashSet};

/// Per-backend storage primitives for authorship notes.
///
/// Object-safe; all methods take `&self` or `&mut self` with no generics.
/// `GitNotesStore<'a>` captures `&'a Repository`; the lifetime is intentionally
/// short (per-call in `notes_api`) and forbids long-lived `Box<dyn AuthorshipNoteStore>`
/// across call boundaries, which the P9.5 plan prohibits anyway.
pub(crate) trait AuthorshipNoteStore {
    fn write_note(&self, commit_sha: &str, content: &str) -> Result<(), GitAiError>;
    fn write_notes_batch(&self, entries: &[(String, String)]) -> Result<(), GitAiError>;
    /// Returns `None` when no note exists for the commit.
    /// Returns `Some(content)` on hit (db or refs depending on impl).
    /// Cross-backend refs-fallback + backfill is expressed in `notes_api`, not here.
    fn read_note(&self, commit_sha: &str) -> Option<String>;
    /// Returns the subset of requested SHAs that have notes, with their content.
    /// Error semantics differ by backend (propagate vs. swallow refs errors);
    /// see `notes_api::read_notes_batch` for the contract.
    fn read_notes_batch(
        &self,
        commit_shas: &[String],
    ) -> Result<HashMap<String, String>, GitAiError>;
    /// Returns the subset of `commit_shas` that have notes in this backend.
    /// Does NOT union with refs; callers do that when needed.
    fn commits_with_notes(&self, commit_shas: &[String]) -> Result<HashSet<String>, GitAiError>;
    /// Returns matching SHAs from this backend only (no cross-store union, no sort).
    /// `notes_api::search_notes` unions db + git results and sorts by date.
    fn search_notes(&self, pattern: &str) -> Result<Vec<String>, GitAiError>;
}

// ---------------------------------------------------------------------------
// Shared db-read helpers (both Sqlite and Http read through the same db path)
// ---------------------------------------------------------------------------

pub(in crate::operations::git) fn db_read_note(commit_sha: &str) -> Option<String> {
    let db = crate::model::repository::notes_db::NotesDatabase::global().ok()?;
    let db_lock = db.lock().ok()?;
    db_lock.get_note(commit_sha).ok().flatten()
}

pub(in crate::operations::git) fn db_read_notes(commit_shas: &[String]) -> HashMap<String, String> {
    let Ok(db) = crate::model::repository::notes_db::NotesDatabase::global() else {
        return HashMap::new();
    };
    let Ok(db_lock) = db.lock() else {
        return HashMap::new();
    };
    let refs: Vec<&str> = commit_shas.iter().map(|s| s.as_str()).collect();
    db_lock.get_notes(&refs).unwrap_or_default()
}

pub(in crate::operations::git) fn db_check_exists(commit_shas: &[String]) -> HashSet<String> {
    db_read_notes(commit_shas).into_keys().collect()
}

// ---------------------------------------------------------------------------
// SqliteNoteStore
// ---------------------------------------------------------------------------

/// Notes backend for `kind = Sqlite` (the default).
///
/// Writes go to `origin='local'` rows (authoritative). Reads serve from the db;
/// on a miss, `notes_api` composes with `GitNotesStore` and calls `backfill_cache`.
///
/// Visibility restricted to `operations::git` so callers must go through
/// `notes_api` public fns rather than constructing this directly.
pub(in crate::operations::git) struct SqliteNoteStore {
    _private: (),
}

impl SqliteNoteStore {
    pub(in crate::operations::git) fn new() -> Self {
        Self { _private: () }
    }

    /// Best-effort backfill of refs-fallback hits into the db cache.
    /// Silently drops db/lock errors — callers treat it as write-through, not
    /// a required step.
    pub(in crate::operations::git) fn backfill_cache(notes: &HashMap<String, String>) {
        if notes.is_empty() {
            return;
        }
        let entries: Vec<(String, String)> = notes
            .iter()
            .map(|(sha, content)| (sha.clone(), content.clone()))
            .collect();
        if let Ok(db) = crate::model::repository::notes_db::NotesDatabase::global()
            && let Ok(mut db_lock) = db.lock()
        {
            let _ = db_lock.cache_synced_notes(&entries);
        }
    }
}

impl AuthorshipNoteStore for SqliteNoteStore {
    fn write_note(&self, commit_sha: &str, content: &str) -> Result<(), GitAiError> {
        let db = crate::model::repository::notes_db::NotesDatabase::global()?;
        let mut db_lock = db
            .lock()
            .map_err(|e| GitAiError::Generic(format!("notes-db lock: {}", e)))?;
        db_lock.upsert_local_note(commit_sha, content)
    }

    fn write_notes_batch(&self, entries: &[(String, String)]) -> Result<(), GitAiError> {
        let db = crate::model::repository::notes_db::NotesDatabase::global()?;
        let mut db_lock = db
            .lock()
            .map_err(|e| GitAiError::Generic(format!("notes-db lock: {}", e)))?;
        db_lock.upsert_local_notes_batch(entries)
    }

    fn read_note(&self, commit_sha: &str) -> Option<String> {
        db_read_note(commit_sha)
    }

    fn read_notes_batch(
        &self,
        commit_shas: &[String],
    ) -> Result<HashMap<String, String>, GitAiError> {
        Ok(db_read_notes(commit_shas))
    }

    fn commits_with_notes(&self, commit_shas: &[String]) -> Result<HashSet<String>, GitAiError> {
        Ok(db_check_exists(commit_shas))
    }

    fn search_notes(&self, pattern: &str) -> Result<Vec<String>, GitAiError> {
        let db = crate::model::repository::notes_db::NotesDatabase::global()?;
        let db_lock = db
            .lock()
            .map_err(|e| GitAiError::Generic(format!("notes-db lock: {}", e)))?;
        db_lock.search_notes_content(pattern)
    }
}

// ---------------------------------------------------------------------------
// HttpNoteStore
// ---------------------------------------------------------------------------

/// Notes backend for `kind = Http`.
///
/// Writes go to `origin='queue'` rows (synced=0, upload-pending) and kick the
/// daemon flush via `telemetry_handle::submit_notes()`. The remote server is the
/// authority; the db is a write-queue + read-cache. On read misses, `notes_api`
/// may optionally compose with remote fetch-and-cache (never backfill-to-refs).
///
/// Visibility restricted to `operations::git` so callers must go through
/// `notes_api` public fns rather than constructing this directly.
pub(in crate::operations::git) struct HttpNoteStore {
    _private: (),
}

impl HttpNoteStore {
    pub(in crate::operations::git) fn new() -> Self {
        Self { _private: () }
    }

    /// Fetch a batch of notes from the remote HTTP backend and write them into the
    /// db as `origin='cache'` (synced=1). Best-effort: individual chunk errors are
    /// logged and skipped. Returns the fetched entries (not necessarily complete).
    pub(in crate::operations::git) fn fetch_and_cache_notes(
        commit_shas: &[String],
    ) -> HashMap<String, String> {
        if commit_shas.is_empty() {
            return HashMap::new();
        }

        use crate::clients::api::client::{ApiClient, ApiContext};
        use crate::config::Config;
        use crate::operations::git::repository::resolve_api_author_identity;

        let cfg = Config::fresh();
        let Some(backend_url) = cfg.notes_backend_url().map(str::to_string) else {
            return HashMap::new();
        };

        let ctx = ApiContext::new(Some(backend_url), resolve_api_author_identity);
        let client = ApiClient::new(ctx);
        if !client.is_logged_in() && !client.has_api_key() {
            return HashMap::new();
        }

        let mut fetched = HashMap::new();
        for chunk in commit_shas.chunks(100) {
            let refs: Vec<&str> = chunk.iter().map(String::as_str).collect();
            match client.read_notes(&refs) {
                Ok(response) => {
                    if response.notes.is_empty() {
                        continue;
                    }
                    let entries: Vec<(String, String)> = response.notes.into_iter().collect();
                    if let Ok(db) = crate::model::repository::notes_db::NotesDatabase::global()
                        && let Ok(mut lock) = db.lock()
                    {
                        let _ = lock.cache_synced_notes(&entries);
                    }
                    fetched.extend(entries);
                }
                Err(e) => {
                    tracing::debug!(%e, "notes batch read from HTTP backend failed");
                }
            }
        }

        fetched
    }
}

impl AuthorshipNoteStore for HttpNoteStore {
    fn write_note(&self, commit_sha: &str, content: &str) -> Result<(), GitAiError> {
        let db = crate::model::repository::notes_db::NotesDatabase::global()?;
        let mut db_lock = db
            .lock()
            .map_err(|e| GitAiError::Generic(format!("notes-db lock: {}", e)))?;
        db_lock.upsert_note(commit_sha, content)?;
        drop(db_lock);
        crate::operations::daemon::telemetry_handle::submit_notes();
        Ok(())
    }

    fn write_notes_batch(&self, entries: &[(String, String)]) -> Result<(), GitAiError> {
        let db = crate::model::repository::notes_db::NotesDatabase::global()?;
        let mut db_lock = db
            .lock()
            .map_err(|e| GitAiError::Generic(format!("notes-db lock: {}", e)))?;
        db_lock.upsert_notes_batch(entries)?;
        drop(db_lock);
        crate::operations::daemon::telemetry_handle::submit_notes();
        Ok(())
    }

    fn read_note(&self, commit_sha: &str) -> Option<String> {
        db_read_note(commit_sha)
        // No backfill on refs miss for Http — notes_api handles refs fallback
        // without backfill (asymmetry vs Sqlite is intentional; see risks in
        // docs/decisions/2026-07-20-layered-architecture-plan.md §P9.5).
    }

    fn read_notes_batch(
        &self,
        commit_shas: &[String],
    ) -> Result<HashMap<String, String>, GitAiError> {
        let mut notes = db_read_notes(commit_shas);

        // Fetch misses from the remote HTTP backend and cache them.
        let missing: Vec<String> = commit_shas
            .iter()
            .filter(|sha| !notes.contains_key(*sha))
            .cloned()
            .collect();
        if !missing.is_empty() {
            notes.extend(Self::fetch_and_cache_notes(&missing));
        }

        Ok(notes)
        // Remaining refs-fallback (errors swallowed) is composed in notes_api.
    }

    fn commits_with_notes(&self, commit_shas: &[String]) -> Result<HashSet<String>, GitAiError> {
        Ok(db_check_exists(commit_shas))
    }

    fn search_notes(&self, pattern: &str) -> Result<Vec<String>, GitAiError> {
        let db = crate::model::repository::notes_db::NotesDatabase::global()?;
        let db_lock = db
            .lock()
            .map_err(|e| GitAiError::Generic(format!("notes-db lock: {}", e)))?;
        db_lock.search_notes_content(pattern)
    }
}

// ---------------------------------------------------------------------------
// GitNotesStore
// ---------------------------------------------------------------------------

/// Notes backend for `kind = GitNotes`.
///
/// `refs/notes/ai` is the sole authority. The db is untouched by this impl.
/// The `&'a Repository` borrow keeps the lifetime bounded to each per-call
/// construction in `notes_api`, which is intentional (prevents long-lived
/// `Box<dyn AuthorshipNoteStore>` storage — forbidden by P9.5 guardrails).
pub(crate) struct GitNotesStore<'a> {
    pub(super) repo: &'a Repository,
}

impl AuthorshipNoteStore for GitNotesStore<'_> {
    fn write_note(&self, commit_sha: &str, content: &str) -> Result<(), GitAiError> {
        crate::operations::git::refs::notes_add(self.repo, commit_sha, content)
    }

    fn write_notes_batch(&self, entries: &[(String, String)]) -> Result<(), GitAiError> {
        crate::operations::git::refs::notes_add_batch(self.repo, entries)
    }

    fn read_note(&self, commit_sha: &str) -> Option<String> {
        crate::operations::git::refs::show_authorship_note(self.repo, commit_sha)
    }

    fn read_notes_batch(
        &self,
        commit_shas: &[String],
    ) -> Result<HashMap<String, String>, GitAiError> {
        crate::operations::git::refs::notes_for_commits(self.repo, commit_shas)
    }

    fn commits_with_notes(&self, commit_shas: &[String]) -> Result<HashSet<String>, GitAiError> {
        crate::operations::git::refs::commits_with_authorship_notes(self.repo, commit_shas)
    }

    fn search_notes(&self, pattern: &str) -> Result<Vec<String>, GitAiError> {
        crate::operations::git::refs::grep_ai_notes(self.repo, pattern)
    }
}
