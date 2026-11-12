//! `git-ai notes migrate` — move existing notes between backends.
//!
//! Targets (`--to <backend>`, defaulting to the configured backend):
//! - `sqlite`: import `refs/notes/ai` into the local notes database as
//!   local-primary rows (idempotent; safe to re-run).
//! - `git-notes`: export local-primary rows from the notes database into
//!   `refs/notes/ai` so attribution can be shared via push/fetch.
//! - `http`: bulk-upload `refs/notes/ai` to the remote HTTP backend in chunks
//!   of 50 and warm the local cache (`synced = 1`).
//!
//! All refs reads use batched plumbing (`git notes list` + `git cat-file
//! --batch`) — a constant git-spawn count regardless of note count.

use crate::clients::api::client::{ApiClient, ApiContext};
use crate::config::{Config, NotesBackendKind};
use crate::error::GitAiError;
use crate::model::api_types::{NoteEntry, NotesUploadRequest};
use crate::model::repository::notes_db::NotesDatabase;
use crate::operations::git::cat_file::{BatchReadPolicy, batch_read_blob_contents_with_policy};
use crate::operations::git::find_repository;
use crate::operations::git::refs::list_all_notes;
use crate::operations::git::repository::resolve_api_author_identity;

/// Entry point for `git-ai notes migrate`.
pub fn handle_notes_migrate(args: &[String]) {
    let mut force = false;
    let mut target: Option<NotesBackendKind> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                return;
            }
            "--force" | "--all" => {
                force = true;
            }
            "--to" => {
                i += 1;
                let value = args.get(i).map(String::as_str).unwrap_or_default();
                target = Some(match value {
                    "sqlite" => NotesBackendKind::Sqlite,
                    "git_notes" | "git-notes" => NotesBackendKind::GitNotes,
                    "http" => NotesBackendKind::Http,
                    other => {
                        eprintln!(
                            "error: invalid --to target '{}': expected sqlite, git-notes, or http",
                            other
                        );
                        std::process::exit(1);
                    }
                });
            }
            other => {
                eprintln!("error: unknown option '{}'", other);
                eprintln!("Run 'git ai notes migrate --help' for usage");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let cfg = Config::fresh();
    let target = target.unwrap_or_else(|| cfg.notes_backend_kind());

    let db = match NotesDatabase::global() {
        Ok(db) => db,
        Err(e) => {
            eprintln!("error: failed to open notes database: {}", e);
            std::process::exit(1);
        }
    };

    let repo = match find_repository(&Vec::<String>::new()) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: not a git repository ({})", e);
            std::process::exit(1);
        }
    };

    match target {
        NotesBackendKind::Sqlite => migrate_refs_to_sqlite(db, &repo),
        NotesBackendKind::GitNotes => migrate_sqlite_to_git_notes(db, &repo),
        NotesBackendKind::Http => migrate_refs_to_http(db, &repo, &cfg, force),
    }
}

/// Import `refs/notes/ai` into the notes database as local-primary rows.
fn migrate_refs_to_sqlite(
    db: &'static std::sync::Mutex<NotesDatabase>,
    repo: &crate::operations::git::repository::Repository,
) {
    eprintln!("Listing notes from refs/notes/ai ...");
    let entries = match read_all_ref_notes(repo) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("error: failed to read notes: {}", e);
            std::process::exit(1);
        }
    };
    if entries.is_empty() {
        eprintln!("No notes found in refs/notes/ai. Nothing to migrate.");
        return;
    }

    match db.lock() {
        Ok(mut lock) => {
            if let Err(e) = lock.upsert_local_notes_batch(&entries) {
                eprintln!("error: failed to write notes database: {}", e);
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: notes-db lock poisoned: {}", e);
            std::process::exit(1);
        }
    }
    eprintln!(
        "Migration complete: {} note(s) imported into the local notes database.",
        entries.len()
    );
}

/// Export local-primary rows from the notes database into `refs/notes/ai`.
fn migrate_sqlite_to_git_notes(
    db: &'static std::sync::Mutex<NotesDatabase>,
    repo: &crate::operations::git::repository::Repository,
) {
    let entries = match db.lock() {
        Ok(lock) => match lock.get_local_notes() {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("error: failed to read notes database: {}", e);
                std::process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("error: notes-db lock poisoned: {}", e);
            std::process::exit(1);
        }
    };
    if entries.is_empty() {
        eprintln!("No local notes found in the notes database. Nothing to export.");
        return;
    }

    if let Err(e) = crate::operations::git::notes_api::export_notes_to_git_refs(repo, &entries) {
        eprintln!("error: failed to write refs/notes/ai: {}", e);
        std::process::exit(1);
    }
    eprintln!(
        "Migration complete: {} note(s) exported to refs/notes/ai.",
        entries.len()
    );
}

/// Read every note in `refs/notes/ai` as `(commit_sha, content)` pairs.
///
/// Commits whose notes have identical content share one blob, so entries are
/// built per listed note, not per blob.
fn read_all_ref_notes(
    repo: &crate::operations::git::repository::Repository,
) -> Result<Vec<(String, String)>, GitAiError> {
    let note_pairs = list_all_notes(repo, "refs/notes/ai")?;
    let blob_shas: Vec<String> = note_pairs.iter().map(|(b, _)| b.clone()).collect();
    let blob_contents =
        batch_read_blob_contents_with_policy(repo, &blob_shas, BatchReadPolicy::Tolerant)?;
    Ok(note_pairs
        .into_iter()
        .filter_map(|(blob_sha, commit_sha)| {
            Some((commit_sha, blob_contents.get(&blob_sha)?.clone()))
        })
        .collect())
}

/// Bulk-upload `refs/notes/ai` to the remote HTTP backend and warm the cache.
fn migrate_refs_to_http(
    db: &'static std::sync::Mutex<NotesDatabase>,
    repo: &crate::operations::git::repository::Repository,
    cfg: &Config,
    force: bool,
) {
    // Build the API client.
    let Some(backend_url) = cfg.notes_backend_url().map(str::to_string) else {
        eprintln!(
            "error: notes_backend.backend_url is not configured.\n\
             \n\
             Set it before running migrate, e.g.:\n\
             \n\
             \x20 git-ai config set notes_backend.backend_url https://your-backend.example.com"
        );
        std::process::exit(1);
    };
    let ctx = ApiContext::new(Some(backend_url), resolve_api_author_identity);
    let client = ApiClient::new(ctx);

    // Skip if not authenticated.
    if !client.is_logged_in() && !client.has_api_key() {
        eprintln!("error: not authenticated. Log in first with `git-ai login` or set an API key.");
        std::process::exit(1);
    }

    eprintln!("Listing notes from refs/notes/ai ...");

    let mut entries = match read_all_ref_notes(repo) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("error: failed to read notes: {}", e);
            std::process::exit(1);
        }
    };

    if entries.is_empty() {
        eprintln!("No notes found in refs/notes/ai. Nothing to migrate.");
        return;
    }

    // Skip entries already confirmed synced (enables safe re-run after interruption).
    // Only skip synced=1 entries — pending (synced=0) entries still need uploading.
    if !force {
        let pre_cached_count = entries.len();
        if let Ok(lock) = db.lock() {
            let all_shas: Vec<&str> = entries.iter().map(|(s, _)| s.as_str()).collect();
            if let Ok(synced) = lock.get_synced_shas(&all_shas) {
                entries.retain(|(sha, _)| !synced.contains(sha));
            }
        }
        if entries.len() < pre_cached_count {
            eprintln!(
                "Skipping {} already-cached note(s).",
                pre_cached_count - entries.len()
            );
        }

        if entries.is_empty() {
            eprintln!("All notes already migrated. Nothing to upload.");
            return;
        }
    }

    eprintln!(
        "Read {} note(s). Uploading in chunks of 50 ...",
        entries.len()
    );

    // Upload in chunks of 50 and cache locally.
    let mut total_uploaded = 0usize;
    let mut total_failed = 0usize;
    let mut cached_entries: Vec<(String, String)> = Vec::new();

    for chunk in entries.chunks(50) {
        let note_entries: Vec<NoteEntry> = chunk
            .iter()
            .map(|(commit_sha, content)| NoteEntry {
                commit_sha: commit_sha.clone(),
                content: content.clone(),
            })
            .collect();

        let chunk_len = note_entries.len();
        let request = NotesUploadRequest {
            entries: note_entries,
        };

        match client.upload_notes(request) {
            Ok(response) => {
                eprintln!(
                    "  chunk: {} uploaded, {} failed",
                    response.success_count, response.failure_count
                );
                total_uploaded += response.success_count;
                total_failed += response.failure_count;

                // Cache the whole chunk best-effort — the server doesn't
                // tell us which specific entries failed.
                cached_entries.extend_from_slice(chunk);
            }
            Err(e) => {
                eprintln!("  error uploading chunk of {}: {}", chunk_len, e);
                total_failed += chunk_len;
            }
        }
    }

    // Write all successfully-uploaded notes to local notes-db with synced = 1.
    if !cached_entries.is_empty() {
        match db.lock() {
            Ok(mut lock) => {
                if let Err(e) = lock.cache_synced_notes(&cached_entries) {
                    eprintln!("warning: failed to cache notes locally: {}", e);
                } else {
                    eprintln!("Cached {} note(s) in local notes-db.", cached_entries.len());
                }
            }
            Err(e) => {
                eprintln!("warning: notes-db lock poisoned: {}", e);
            }
        }
    }

    // Summary.
    eprintln!();
    if total_failed == 0 {
        eprintln!(
            "Migration complete: {} note(s) uploaded successfully.",
            total_uploaded
        );
    } else {
        eprintln!(
            "Migration finished: {} uploaded, {} failed.",
            total_uploaded, total_failed
        );
        if total_failed > 0 {
            std::process::exit(1);
        }
    }
}

fn print_help() {
    eprintln!("git ai notes migrate - Bulk-upload existing git notes to the HTTP backend");
    eprintln!();
    eprintln!("Usage: git ai notes migrate [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --force, --all  Re-upload all notes even if already cached locally.");
    eprintln!("                  Useful when migrating to a new backend URL.");
    eprintln!("  -h, --help      Show this help message");
    eprintln!();
    eprintln!("Description:");
    eprintln!("  Reads all notes from refs/notes/ai, uploads them to the configured HTTP");
    eprintln!("  notes backend (in chunks of 50), and caches them locally in notes-db");
    eprintln!("  with synced = 1 so the local cache is warm immediately.");
    eprintln!();
    eprintln!("  This command requires notes_backend.kind = http. Set it with:");
    eprintln!("    git-ai config set notes_backend.kind http");
    eprintln!();
    eprintln!("  You must be logged in or have an API key configured.");
}

// --- Tests ---

#[cfg(test)]
#[path = "notes_migrate_tests.rs"]
mod tests;
