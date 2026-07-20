use serde::{Deserialize, Serialize};

/// Which backend to use for storing authorship notes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotesBackendKind {
    /// Default: store notes in the local SQLite notes database. Reads fall
    /// back to refs/notes/ai so pre-existing repos keep working.
    #[default]
    Sqlite,
    /// Store notes in git refs/notes/ai (shareable with teammates via
    /// push/fetch of the notes ref).
    GitNotes,
    /// HTTP backend: queue writes to notes-db, flush via daemon, reads from cache
    Http,
}

impl NotesBackendKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            NotesBackendKind::Sqlite => "sqlite",
            NotesBackendKind::GitNotes => "git_notes",
            NotesBackendKind::Http => "http",
        }
    }
}

impl std::fmt::Display for NotesBackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for the notes backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NotesBackendConfig {
    #[serde(default)]
    pub kind: NotesBackendKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend_url: Option<String>,
}
