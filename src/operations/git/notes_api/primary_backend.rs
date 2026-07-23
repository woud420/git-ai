use crate::config::NotesBackendKind;
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::git::notes_store::{AuthorshipNoteStore, HttpNoteStore, SqliteNoteStore};
use crate::operations::git::repository::Repository;

/// Read authorship only from the configured primary store.
///
/// Unlike the public notes API, this does not cross backend boundaries:
/// Sqlite and Http read only their local database/cache, while GitNotes reads
/// only `refs/notes/ai`. Polling callers can therefore resolve the backend once
/// without repeatedly exercising fallback paths.
pub(crate) fn read_authorship(
    repo: &Repository,
    commit_sha: &str,
    backend_kind: NotesBackendKind,
) -> Option<AuthorshipLog> {
    match backend_kind {
        NotesBackendKind::Sqlite => SqliteNoteStore::new()
            .read_note(commit_sha)
            .and_then(deserialize_authorship),
        NotesBackendKind::Http => HttpNoteStore::new()
            .read_note(commit_sha)
            .and_then(deserialize_authorship),
        NotesBackendKind::GitNotes => {
            crate::operations::git::refs::get_authorship(repo, commit_sha)
        }
    }
}

pub(super) fn deserialize_authorship(content: String) -> Option<AuthorshipLog> {
    AuthorshipLog::deserialize_from_string(&content)
        .map_err(|error| tracing::debug!("notes deserialization error: {}", error))
        .ok()
}
