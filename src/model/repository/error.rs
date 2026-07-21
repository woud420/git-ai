use crate::error::GitAiError;
use crate::error::Retryability;
use std::fmt;

/// Structured error type for repository persistence operations.
///
/// Carries enough context for diagnostics without holding non-`Clone` types
/// from rusqlite or std. The `GitAiError::Persistence` variant wraps this
/// so every existing `pub fn … -> Result<_, GitAiError>` boundary compiles
/// unchanged via `?` auto-conversion.
#[derive(Debug, Clone)]
pub enum PersistenceError {
    /// A SQLite operation failed. Stores a cloneable projection of the
    /// rusqlite error (code + message) rather than the error itself, so
    /// `Clone` is non-lossy.
    Sqlite {
        db: &'static str,
        operation: &'static str,
        code: Option<rusqlite::ffi::ErrorCode>,
        message: String,
    },
    /// A `Mutex` was poisoned, most likely because another thread panicked
    /// while holding the lock.
    LockPoisoned { what: &'static str },
    /// The on-disk schema version is ahead of what this binary supports.
    Migration {
        db: &'static str,
        found: String,
        supported: String,
    },
    /// No migration is registered for the requested version transition
    /// (a defensive dev-invariant guard, unlike `Migration`).
    NoMigrationPath {
        db: &'static str,
        from: String,
        to: String,
    },
    /// A filesystem I/O operation failed.
    Io {
        operation: &'static str,
        path: String,
        kind: std::io::ErrorKind,
        message: String,
    },
}

impl PersistenceError {
    /// Construct a `Sqlite` variant from a raw `rusqlite::Error`.
    pub fn sqlite(db: &'static str, operation: &'static str, err: &rusqlite::Error) -> Self {
        let code = if let rusqlite::Error::SqliteFailure(ref ffi, _) = *err {
            Some(ffi.code)
        } else {
            None
        };
        PersistenceError::Sqlite {
            db,
            operation,
            code,
            message: err.to_string(),
        }
    }

    /// Construct the standard "home directory not found" error.
    pub(crate) fn home_dir_not_found() -> Self {
        PersistenceError::Io {
            operation: "home directory lookup",
            path: String::new(),
            kind: std::io::ErrorKind::NotFound,
            message: "Could not determine home directory".to_string(),
        }
    }

    /// Construct a schema-version-mismatch error as a `GitAiError`.
    ///
    /// Returns `GitAiError` directly to keep call sites compact (avoids a
    /// trailing `.into()` that pushes long lines over the formatter's width).
    pub(crate) fn schema_version(db: &'static str, found: usize, supported: usize) -> GitAiError {
        GitAiError::Persistence(PersistenceError::Migration {
            db,
            found: found.to_string(),
            supported: supported.to_string(),
        })
    }

    /// Construct a missing-migration-path error as a `GitAiError`.
    ///
    /// Distinct from [`Self::schema_version`]: this is the defensive guard for a
    /// version gap with no registered migration, not a DB written by a newer binary.
    pub(crate) fn no_migration_path(db: &'static str, from: usize, to: usize) -> GitAiError {
        GitAiError::Persistence(PersistenceError::NoMigrationPath {
            db,
            from: from.to_string(),
            to: to.to_string(),
        })
    }

    /// Returns whether this error is worth retrying.
    ///
    /// Mirrors the `SQLITE_BUSY`/`SQLITE_LOCKED` classification in
    /// `src/operations/streams/agents/copilot_otel.rs::map_sqlite_error`.
    pub fn retryability(&self) -> Retryability {
        if let PersistenceError::Sqlite {
            code: Some(code), ..
        } = self
        {
            if matches!(
                code,
                rusqlite::ffi::ErrorCode::DatabaseBusy | rusqlite::ffi::ErrorCode::DatabaseLocked
            ) {
                return Retryability::Retryable { retry_after: None };
            }
        }
        Retryability::Terminal
    }
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistenceError::Sqlite {
                db,
                operation,
                message,
                ..
            } => write!(f, "{} {}: {}", db, operation, message),
            PersistenceError::LockPoisoned { what } => write!(f, "{} lock poisoned", what),
            PersistenceError::Migration {
                db,
                found,
                supported,
            } => {
                write!(
                    f,
                    "{}: schema version {} is newer than supported version {}; upgrade git-ai to the latest version",
                    db, found, supported
                )
            }
            PersistenceError::NoMigrationPath { db, from, to } => {
                write!(
                    f,
                    "{}: no migration path from version {} to {}",
                    db, from, to
                )
            }
            PersistenceError::Io {
                operation,
                path,
                message,
                ..
            } => {
                if path.is_empty() {
                    write!(f, "{}: {}", operation, message)
                } else {
                    write!(f, "{} ({}): {}", operation, path, message)
                }
            }
        }
    }
}

impl From<PersistenceError> for GitAiError {
    fn from(e: PersistenceError) -> Self {
        GitAiError::Persistence(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    // --- Display exactness ---

    #[test]
    fn sqlite_display() {
        let e = PersistenceError::Sqlite {
            db: "notes",
            operation: "insert",
            code: None,
            message: "table notes has no column named foo".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "notes insert: table notes has no column named foo"
        );
    }

    #[test]
    fn lock_poisoned_display() {
        let e = PersistenceError::LockPoisoned { what: "notes-db" };
        assert_eq!(e.to_string(), "notes-db lock poisoned");
    }

    #[test]
    fn migration_display() {
        let e = PersistenceError::Migration {
            db: "metrics",
            found: "9".to_string(),
            supported: "5".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "metrics: schema version 9 is newer than supported version 5; upgrade git-ai to the latest version"
        );
    }

    #[test]
    fn no_migration_path_display_and_terminal() {
        let e = PersistenceError::NoMigrationPath {
            db: "notes",
            from: "3".to_string(),
            to: "4".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "notes: no migration path from version 3 to 4"
        );
        assert!(matches!(e.retryability(), Retryability::Terminal));
    }

    #[test]
    fn io_display_with_path() {
        let e = PersistenceError::Io {
            operation: "open",
            path: "/home/user/.git-ai/db".to_string(),
            kind: ErrorKind::PermissionDenied,
            message: "Permission denied (os error 13)".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "open (/home/user/.git-ai/db): Permission denied (os error 13)"
        );
    }

    #[test]
    fn io_display_without_path() {
        let e = PersistenceError::Io {
            operation: "home directory lookup",
            path: String::new(),
            kind: ErrorKind::NotFound,
            message: "Could not determine home directory".to_string(),
        };
        assert_eq!(
            e.to_string(),
            "home directory lookup: Could not determine home directory"
        );
    }

    // --- Retryability ---

    #[test]
    fn sqlite_busy_is_retryable() {
        let e = PersistenceError::Sqlite {
            db: "notes",
            operation: "insert",
            code: Some(rusqlite::ffi::ErrorCode::DatabaseBusy),
            message: "database is locked".to_string(),
        };
        assert!(matches!(e.retryability(), Retryability::Retryable { .. }));
    }

    #[test]
    fn sqlite_locked_is_retryable() {
        let e = PersistenceError::Sqlite {
            db: "notes",
            operation: "update",
            code: Some(rusqlite::ffi::ErrorCode::DatabaseLocked),
            message: "database is locked".to_string(),
        };
        assert!(matches!(e.retryability(), Retryability::Retryable { .. }));
    }

    #[test]
    fn sqlite_corrupt_is_terminal() {
        let e = PersistenceError::Sqlite {
            db: "internal",
            operation: "query",
            code: Some(rusqlite::ffi::ErrorCode::DatabaseCorrupt),
            message: "database disk image is malformed".to_string(),
        };
        assert!(matches!(e.retryability(), Retryability::Terminal));
    }

    #[test]
    fn sqlite_no_code_is_terminal() {
        let e = PersistenceError::Sqlite {
            db: "metrics",
            operation: "insert",
            code: None,
            message: "some error".to_string(),
        };
        assert!(matches!(e.retryability(), Retryability::Terminal));
    }

    #[test]
    fn lock_poisoned_is_terminal() {
        let e = PersistenceError::LockPoisoned { what: "db" };
        assert!(matches!(e.retryability(), Retryability::Terminal));
    }

    #[test]
    fn migration_is_terminal() {
        let e = PersistenceError::Migration {
            db: "notes",
            found: "99".to_string(),
            supported: "5".to_string(),
        };
        assert!(matches!(e.retryability(), Retryability::Terminal));
    }

    // --- From-bridge Display passthrough ---

    #[test]
    fn from_bridge_display_matches_inner() {
        let inner = PersistenceError::LockPoisoned {
            what: "bash-history",
        };
        let inner_display = inner.to_string();
        let as_git_ai: GitAiError = inner.into();
        // GitAiError::Persistence delegates Display to inner with no prefix
        assert_eq!(as_git_ai.to_string(), inner_display);
    }

    #[test]
    fn from_bridge_produces_persistence_variant() {
        let inner = PersistenceError::Migration {
            db: "metrics",
            found: "9".to_string(),
            supported: "5".to_string(),
        };
        let as_git_ai: GitAiError = inner.into();
        assert!(matches!(as_git_ai, GitAiError::Persistence(_)));
    }

    // --- Non-lossy Clone ---

    #[test]
    fn clone_sqlite_preserves_code() {
        let e = PersistenceError::Sqlite {
            db: "notes",
            operation: "insert",
            code: Some(rusqlite::ffi::ErrorCode::DatabaseBusy),
            message: "database is locked".to_string(),
        };
        let cloned = e.clone();
        // The clone must still know it's Retryable — if Clone were lossy
        // (degrading to Generic) retryability would be lost.
        assert!(matches!(
            cloned.retryability(),
            Retryability::Retryable { .. }
        ));
        if let PersistenceError::Sqlite { code, .. } = cloned {
            assert_eq!(code, Some(rusqlite::ffi::ErrorCode::DatabaseBusy));
        } else {
            panic!("clone changed variant");
        }
    }

    #[test]
    fn clone_lock_poisoned() {
        let e = PersistenceError::LockPoisoned { what: "metrics" };
        let cloned = e.clone();
        assert_eq!(e.to_string(), cloned.to_string());
        assert!(matches!(cloned, PersistenceError::LockPoisoned { .. }));
    }

    #[test]
    fn clone_via_git_ai_error_is_non_lossy() {
        // Wrap in GitAiError and clone — must stay Persistence, not degrade to Generic.
        let inner = PersistenceError::Sqlite {
            db: "internal",
            operation: "migrate",
            code: Some(rusqlite::ffi::ErrorCode::DatabaseBusy),
            message: "busy".to_string(),
        };
        let wrapped: GitAiError = inner.into();
        let cloned = wrapped.clone();
        assert!(
            matches!(cloned, GitAiError::Persistence(_)),
            "Persistence variant must survive Clone without degrading to Generic"
        );
    }
}
