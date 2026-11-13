use super::*;
use crate::operations::git::refs::{notes_for_commits_utf8, parse_reference_as_authorship_log_v3};

// Keep single-note deserialization diagnostics byte-stable for existing callers.
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

/// Batch the local-only reads used by blame. Unusable individual notes are omitted;
/// a failed batch transport is returned instead of retrying once per commit.
/// Unlike read_notes_batch, HTTP cache misses never start a network request.
pub fn read_authorship_v3_batch(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashMap<String, AuthorshipLog>, GitAiError> {
    read_batch_for_backend(repo, commit_shas, Config::fresh().notes_backend_kind())
}

/// Batched [`read_note`]: the same local-only reads, with the same omissions.
/// A note that cannot be read is left out; a failed batch transport is
/// returned instead of retrying once per commit.
pub fn read_local_notes_batch(
    repo: &Repository,
    commit_shas: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    let backend = Config::fresh().notes_backend_kind();
    let (mut notes, refs) = read_local_notes(repo, commit_shas, backend)?;
    notes.extend(refs);
    Ok(notes)
}

/// Note content by commit SHA.
type Notes = HashMap<String, String>;

/// Note contents from the notes database, and from `refs/notes/ai` for the
/// commits the database lacks (always, on the GitNotes backend).
fn read_local_notes(
    repo: &Repository,
    commit_shas: &[String],
    backend: NotesBackendKind,
) -> Result<(Notes, Notes), GitAiError> {
    if commit_shas.is_empty() {
        return Ok(Default::default());
    }
    let commit_shas: Vec<String> = commit_shas
        .iter()
        .cloned()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let cached = match backend {
        NotesBackendKind::GitNotes => HashMap::new(),
        NotesBackendKind::Sqlite | NotesBackendKind::Http => db_read_notes(&commit_shas),
    };
    let missing: Vec<String> = commit_shas
        .iter()
        .filter(|sha| !cached.contains_key(*sha))
        .cloned()
        .collect();
    let refs = notes_for_commits_utf8(repo, &missing)?;
    // Single-note reads trim before SQLite backfill, even when parsing later fails.
    let refs: HashMap<String, String> = refs
        .into_iter()
        .filter_map(|(sha, content)| {
            let content = content.trim();
            (!content.is_empty()).then(|| (sha, content.to_owned()))
        })
        .collect();
    if backend == NotesBackendKind::Sqlite {
        SqliteNoteStore::backfill_cache(&refs);
    }
    Ok((cached, refs))
}

fn read_batch_for_backend(
    repo: &Repository,
    commit_shas: &[String],
    backend: NotesBackendKind,
) -> Result<HashMap<String, AuthorshipLog>, GitAiError> {
    let (cached, refs) = read_local_notes(repo, commit_shas, backend)?;
    let mut logs: HashMap<String, AuthorshipLog> = cached
        .into_iter()
        .filter_map(|(sha, content)| {
            AuthorshipLog::deserialize_from_string(&content)
                .ok()
                .map(|log| (sha, log))
        })
        .collect();
    for (sha, content) in refs {
        // SQLite's single-note path parses backfilled content like a cache hit;
        // GitNotes and HTTP refs fallback validate version and normalize the base.
        let log = if backend == NotesBackendKind::Sqlite {
            AuthorshipLog::deserialize_from_string(&content).ok()
        } else {
            parse_reference_as_authorship_log_v3(&content, &sha).ok()
        };
        if let Some(log) = log {
            logs.insert(sha, log);
        }
    }
    Ok(logs)
}

#[cfg(test)]
mod tests;
