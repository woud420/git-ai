//! Notes-database to HTTP backend flush logic.

use super::TelemetryStores;
use crate::clients::api::{ApiClient, ApiContext};
use crate::config::{Config, NotesBackendKind};
use crate::model::api_types::{NoteEntry, NotesUploadRequest};
use crate::operations::git::repository::resolve_api_author_identity;

impl From<crate::model::repository::notes_db::PendingNote> for NoteEntry {
    fn from(p: crate::model::repository::notes_db::PendingNote) -> Self {
        Self {
            commit_sha: p.commit_sha,
            content: p.content,
        }
    }
}

/// Flush pending notes from `notes-db` to the remote HTTP backend.
pub(super) fn flush_notes_with(stores: Option<TelemetryStores>) {
    let cfg = Config::fresh();
    if cfg.notes_backend_kind() != NotesBackendKind::Http {
        tracing::debug!("notes: skipping flush, backend is not Http");
        return;
    }

    let Some(backend_url) = cfg.notes_backend_url().map(str::to_string) else {
        tracing::debug!("notes: skipping flush, notes_backend.backend_url is not configured");
        return;
    };
    let context = ApiContext::new(Some(backend_url), resolve_api_author_identity);
    let client = ApiClient::new(context);

    if !client.is_logged_in() && !client.has_api_key() {
        tracing::debug!("notes: skipping flush, not authenticated");
        return;
    }
    let notes_db = if let Some(s) = stores {
        s.notes
    } else {
        match crate::model::repository::notes_db::NotesDatabase::global() {
            Ok(db) => db,
            Err(e) => {
                tracing::warn!(%e, "notes: failed to get notes DB");
                return;
            }
        }
    };
    // Collect shas before consuming rows to avoid double-cloning the payloads.
    let Ok(mut lock) = notes_db.lock() else {
        return;
    };
    let pending = match lock.dequeue_pending(50) {
        Ok(rows) if !rows.is_empty() => rows,
        Ok(_) => return,
        Err(e) => {
            tracing::warn!("notes: failed to dequeue pending rows: {e}");
            return;
        }
    };
    drop(lock);
    let commit_shas: Vec<String> = pending.iter().map(|p| p.commit_sha.clone()).collect();
    let request = NotesUploadRequest {
        entries: pending.into_iter().map(Into::into).collect(),
    };
    match client.upload_notes(request) {
        Ok(resp) => {
            tracing::debug!(
                success = resp.success_count,
                failure = resp.failure_count,
                "notes: uploaded batch"
            );
            if let Ok(mut lock) = notes_db.lock() {
                if resp.failure_count == 0 {
                    let _ = lock.mark_synced(&commit_shas);
                } else {
                    // Server reported partial failures; retry the entire batch next cycle.
                    let _ = lock.mark_failed(
                        &commit_shas,
                        &format!(
                            "partial failure: {}/{} entries failed",
                            resp.failure_count,
                            commit_shas.len()
                        ),
                    );
                }
            }
        }
        Err(e) => {
            tracing::warn!(%e, "notes: upload error");
            if let Ok(mut lock) = notes_db.lock() {
                let _ = lock.mark_failed(&commit_shas, &e.to_string());
            }
        }
    }

    // Opportunistic cache eviction (~every 5 minutes at 3s flush interval).
    use std::sync::atomic::{AtomicU32, Ordering};
    static FLUSH_COUNT: AtomicU32 = AtomicU32::new(0);
    if FLUSH_COUNT
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(100)
        && let Ok(mut lock) = notes_db.lock()
    {
        let _ = lock.evict_stale_cache(10_000, 90 * 24 * 3600);
    }
}

/// Flush notes via the global singleton (fallback for the FlushNotes control handler).
pub fn flush_notes_global() {
    flush_notes_with(None);
}

pub(super) fn flush_notes_for_await(stores: TelemetryStores) -> usize {
    let cfg = Config::fresh();
    if cfg.notes_backend_kind() != NotesBackendKind::Http || cfg.notes_backend_url().is_none() {
        return 0;
    }
    let backend_url = cfg.notes_backend_url().map(str::to_string);
    let client = ApiClient::new(ApiContext::new(backend_url, resolve_api_author_identity));
    if !client.is_logged_in() && !client.has_api_key() {
        return 0;
    }
    for _ in 0..1_000 {
        if count_pending_notes_for_await(stores) == 0 {
            return 0;
        }
        flush_notes_with(Some(stores));
    }
    count_pending_notes_for_await(stores)
}

pub(super) fn count_pending_notes_for_await(stores: TelemetryStores) -> usize {
    let Ok(lock) = stores.notes.lock() else {
        return 0;
    };
    lock.count_pending_uploadable().unwrap_or(0)
}
