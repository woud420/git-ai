use std::fmt;

/// Whether an operation that produced an error is worth retrying.
///
/// Grounded in `StreamError::Transient` (src/model/stream_types.rs) and the
/// `SQLITE_BUSY` classification in copilot_otel.rs — the only existing
/// retryability semantics in the codebase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retryability {
    /// The operation may succeed if retried. `retry_after` is a suggested
    /// minimum delay; callers that manage their own backoff may ignore it.
    Retryable {
        retry_after: Option<std::time::Duration>,
    },
    /// The error is permanent; retrying would produce the same result.
    Terminal,
}

#[derive(Debug)]
pub enum GitAiError {
    IoError(std::io::Error),
    /// Errors from invoking the git CLI that exited with a non-zero status
    GitCliError {
        code: Option<i32>,
        stderr: String,
        args: Vec<String>,
    },
    /// Errors from  Gix
    GixError(String),
    JsonError(serde_json::Error),
    Utf8Error(std::str::Utf8Error),
    FromUtf8Error(std::string::FromUtf8Error),
    PresetError(String),
    SqliteError(rusqlite::Error),
    Generic(String),
    /// Structured persistence-layer error. Display delegates to the inner
    /// type with no added prefix, so the observable message text is
    /// controlled by `PersistenceError::fmt`.
    Persistence(crate::model::repository::error::PersistenceError),
}

impl fmt::Display for GitAiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitAiError::IoError(e) => write!(f, "IO error: {}", e),
            GitAiError::GitCliError { code, stderr, args } => match code {
                Some(c) => write!(
                    f,
                    "Git CLI ({}) failed with exit code {}: {}",
                    args.join(" "),
                    c,
                    stderr
                ),
                None => write!(f, "Git CLI ({}) failed: {}", args.join(" "), stderr),
            },
            GitAiError::JsonError(e) => write!(f, "JSON error: {}", e),
            GitAiError::Utf8Error(e) => write!(f, "UTF-8 error: {}", e),
            GitAiError::FromUtf8Error(e) => write!(f, "From UTF-8 error: {}", e),
            GitAiError::PresetError(e) => write!(f, "{}", e),
            GitAiError::SqliteError(e) => write!(f, "SQLite error: {}", e),
            GitAiError::Generic(e) => write!(f, "Generic error: {}", e),
            GitAiError::GixError(e) => write!(f, "Gix error: {}", e),
            GitAiError::Persistence(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for GitAiError {}

impl From<std::io::Error> for GitAiError {
    fn from(err: std::io::Error) -> Self {
        GitAiError::IoError(err)
    }
}

impl From<serde_json::Error> for GitAiError {
    fn from(err: serde_json::Error) -> Self {
        GitAiError::JsonError(err)
    }
}

impl From<std::str::Utf8Error> for GitAiError {
    fn from(err: std::str::Utf8Error) -> Self {
        GitAiError::Utf8Error(err)
    }
}

impl From<std::string::FromUtf8Error> for GitAiError {
    fn from(err: std::string::FromUtf8Error) -> Self {
        GitAiError::FromUtf8Error(err)
    }
}

impl From<rusqlite::Error> for GitAiError {
    fn from(err: rusqlite::Error) -> Self {
        GitAiError::SqliteError(err)
    }
}

impl Clone for GitAiError {
    fn clone(&self) -> Self {
        match self {
            GitAiError::IoError(e) => {
                GitAiError::IoError(std::io::Error::new(e.kind(), e.to_string()))
            }
            GitAiError::GitCliError { code, stderr, args } => GitAiError::GitCliError {
                code: *code,
                stderr: stderr.clone(),
                args: args.clone(),
            },
            GitAiError::JsonError(e) => GitAiError::Generic(format!("JSON error: {}", e)),
            GitAiError::Utf8Error(e) => GitAiError::Utf8Error(*e),
            GitAiError::FromUtf8Error(e) => GitAiError::FromUtf8Error(e.clone()),
            GitAiError::PresetError(s) => GitAiError::PresetError(s.clone()),
            GitAiError::SqliteError(e) => GitAiError::Generic(format!("SQLite error: {}", e)),
            GitAiError::Generic(s) => GitAiError::Generic(s.clone()),
            GitAiError::GixError(e) => GitAiError::Generic(format!("Gix error: {}", e)),
            // PersistenceError derives Clone, so this is non-lossy.
            GitAiError::Persistence(e) => GitAiError::Persistence(e.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
