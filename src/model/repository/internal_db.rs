// DEPRECATED: The internal DB is deprecated and in the process of being removed.
// It has been superseded by use-case-specific databases.

use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use dirs;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Current schema version (must match MIGRATIONS.len())
const SCHEMA_VERSION: usize = 3;

/// Database migrations - each migration upgrades the schema by one version
/// Migration at index N upgrades from version N to version N+1
const MIGRATIONS: &[&str] = &[
    // Migration 0 -> 1: Initial schema with prompts table
    r#"
    CREATE TABLE IF NOT EXISTS prompts (
        id TEXT PRIMARY KEY NOT NULL,
        workdir TEXT,
        tool TEXT NOT NULL,
        model TEXT NOT NULL,
        external_thread_id TEXT NOT NULL,
        messages TEXT NOT NULL,
        commit_sha TEXT,
        agent_metadata TEXT,
        human_author TEXT,
        total_additions INTEGER,
        total_deletions INTEGER,
        accepted_lines INTEGER,
        overridden_lines INTEGER,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_prompts_tool
        ON prompts(tool);
    CREATE INDEX IF NOT EXISTS idx_prompts_external_thread_id
        ON prompts(external_thread_id);
    CREATE INDEX IF NOT EXISTS idx_prompts_workdir
        ON prompts(workdir);
    CREATE INDEX IF NOT EXISTS idx_prompts_commit_sha
        ON prompts(commit_sha);
    CREATE INDEX IF NOT EXISTS idx_prompts_updated_at
        ON prompts(updated_at);
    "#,
    // Migration 1 -> 2: Add CAS sync queue
    r#"
    CREATE TABLE IF NOT EXISTS cas_sync_queue (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT NOT NULL UNIQUE,
        data TEXT NOT NULL,
        metadata TEXT NOT NULL DEFAULT '{}',
        status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'processing')),
        attempts INTEGER NOT NULL DEFAULT 0,
        last_sync_error TEXT,
        last_sync_at INTEGER,
        next_retry_at INTEGER NOT NULL,
        processing_started_at INTEGER,
        created_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_cas_sync_queue_status_retry
        ON cas_sync_queue(status, next_retry_at);
    CREATE INDEX IF NOT EXISTS idx_cas_sync_queue_hash
        ON cas_sync_queue(hash);
    CREATE INDEX IF NOT EXISTS idx_cas_sync_queue_stale_processing
        ON cas_sync_queue(processing_started_at) WHERE status = 'processing';
    "#,
    // Migration 2 -> 3: Add CAS cache for fetched prompts
    r#"
    CREATE TABLE IF NOT EXISTS cas_cache (
        hash TEXT PRIMARY KEY NOT NULL,
        messages TEXT NOT NULL,
        cached_at INTEGER NOT NULL
    );
    "#,
];

/// Global database singleton
static INTERNAL_DB: OnceLock<Mutex<InternalDatabase>> = OnceLock::new();

/// Database wrapper for internal git-ai storage
pub struct InternalDatabase {
    conn: Connection,
    _db_path: PathBuf,
}

impl InternalDatabase {
    /// Get or initialize the global database
    pub fn global() -> Result<&'static Mutex<InternalDatabase>, GitAiError> {
        // Use get_or_init (stable) instead of get_or_try_init (unstable)
        // Errors during initialization will be logged and returned as Err
        let db_mutex = INTERNAL_DB.get_or_init(|| {
            match Self::new() {
                Ok(db) => Mutex::new(db),
                Err(e) => {
                    // Log error during initialization
                    eprintln!("[Error] Failed to initialize internal database: {}", e);
                    crate::observability::log_error(
                        &e,
                        Some(serde_json::json!({"function": "InternalDatabase::global"})),
                    );
                    // Create a dummy connection that will fail on any operation
                    // This allows the program to continue even if DB init fails
                    let temp_path = std::env::temp_dir().join("git-ai-db-failed");
                    let conn =
                        crate::model::repository::sqlite::open_with_memory_limits(&temp_path)
                            .expect("Failed to create temp DB");
                    Mutex::new(InternalDatabase {
                        conn,
                        _db_path: temp_path,
                    })
                }
            }
        });

        Ok(db_mutex)
    }

    /// Start database initialization in a background thread.
    /// This allows the main thread to continue with other work while
    /// the database connection and schema migrations are prepared.
    ///
    /// The OnceLock guarantees thread-safe initialization - if warmup
    /// completes before any caller needs the DB, they get instant access.
    /// If a caller needs DB before warmup completes, they wait normally.
    pub fn warmup() {
        std::thread::spawn(|| {
            if let Err(e) = Self::global() {
                tracing::debug!("DB warmup failed: {}", e);
            }
        });
    }

    /// Create a new database connection
    fn new() -> Result<Self, GitAiError> {
        let db_path = Self::database_path()?;
        crate::model::repository::sqlite::open_at_path(&db_path, |conn| {
            let mut db = Self {
                conn,
                _db_path: db_path.clone(),
            };
            db.initialize_schema()?;
            Ok(db)
        })
    }

    /// Get database path: ~/.git-ai/internal/db
    /// In test mode, can be overridden via GIT_AI_TEST_DB_PATH environment variable.
    /// We also support GITAI_TEST_DB_PATH because some git hook execution paths
    /// may scrub custom GIT_* variables.
    fn database_path() -> Result<PathBuf, GitAiError> {
        // Allow test override via environment variable
        #[cfg(any(test, feature = "test-support"))]
        if let Ok(test_path) =
            std::env::var("GIT_AI_TEST_DB_PATH").or_else(|_| std::env::var("GITAI_TEST_DB_PATH"))
        {
            return Ok(PathBuf::from(test_path));
        }

        let home = dirs::home_dir().ok_or_else(PersistenceError::home_dir_not_found)?;
        Ok(home.join(".git-ai").join("internal").join("db"))
    }

    /// Initialize schema and handle migrations
    /// This is the ONLY place where schema changes should be made
    /// Failures are FATAL - the program cannot continue without a valid database
    fn initialize_schema(&mut self) -> Result<(), GitAiError> {
        use crate::model::repository::sqlite;

        // FAST PATH: the database is already at the current version, which
        // avoids schema operations on every process start. A newer version is
        // forward-compatible: an older binary can still read/write the tables
        // it knows even if a newer binary added more, so skip migrations.
        if sqlite::read_schema_version(&self.conn).is_some_and(|v| v >= SCHEMA_VERSION) {
            return Ok(());
        }

        // No version yet (brand new database) or an older one: apply all
        // missing migrations sequentially (FATAL on error).
        sqlite::apply_migrations(
            &mut self.conn,
            "internal",
            SCHEMA_VERSION,
            MIGRATIONS,
            |_, _| Ok(()),
        )
    }

    /// Delete CAS sync records by their content hashes (used by daemon after successful upload).
    pub fn delete_cas_by_hashes(&mut self, hashes: &[String]) -> Result<usize, GitAiError> {
        if hashes.is_empty() {
            return Ok(0);
        }
        let sql = format!(
            "DELETE FROM cas_sync_queue WHERE hash IN ({})",
            crate::model::repository::sqlite::in_placeholders(hashes.len())
        );
        Ok(self
            .conn
            .execute(&sql, rusqlite::params_from_iter(hashes))?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::repository::sqlite::assert_persisted_schema_version;
    use tempfile::TempDir;

    fn create_test_db() -> (InternalDatabase, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();

        let mut db = InternalDatabase {
            conn,
            _db_path: db_path.clone(),
        };
        db.initialize_schema().unwrap();

        (db, temp_dir)
    }

    #[test]
    fn test_initialize_schema() {
        let (db, _temp_dir) = create_test_db();

        // Verify tables exist
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='prompts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify schema_metadata exists
        assert_persisted_schema_version(&db.conn, "3");
    }

    #[test]
    fn test_initialize_schema_handles_preexisting_cas_cache_table() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("concurrent-init.db");
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();

        // Simulate a partial migration state from a concurrent process:
        // schema version indicates cas_cache is missing, but the table already exists.
        conn.execute_batch(
            r#"
            CREATE TABLE schema_metadata (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            INSERT INTO schema_metadata (key, value) VALUES ('version', '2');
            CREATE TABLE cas_cache (
                hash TEXT PRIMARY KEY NOT NULL,
                messages TEXT NOT NULL,
                cached_at INTEGER NOT NULL
            );
            "#,
        )
        .unwrap();

        let mut db = InternalDatabase {
            conn,
            _db_path: db_path,
        };
        db.initialize_schema().unwrap();

        assert_persisted_schema_version(&db.conn, "3");
    }

    #[test]
    fn test_initialize_schema_accepts_newer_schema_version() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("newer.db");
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();

        // Written by a newer binary: forward-compatible, so neither an error
        // nor a downgrade of the recorded version.
        conn.execute_batch(
            r#"
            CREATE TABLE schema_metadata (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            INSERT INTO schema_metadata (key, value) VALUES ('version', '99');
            "#,
        )
        .unwrap();

        let mut db = InternalDatabase {
            conn,
            _db_path: db_path,
        };
        db.initialize_schema().unwrap();

        assert_persisted_schema_version(&db.conn, "99");
    }

    #[test]
    fn test_database_path() {
        let override_path = std::env::var("GIT_AI_TEST_DB_PATH").ok();
        let path = InternalDatabase::database_path().unwrap();
        if let Some(override_path) = override_path {
            assert_eq!(path, PathBuf::from(override_path));
        } else {
            assert!(path.to_string_lossy().contains(".git-ai"));
            assert!(path.to_string_lossy().contains("internal"));
            assert!(path.to_string_lossy().ends_with("db"));
        }
    }

    // CAS sync queue tests

    #[test]
    fn test_cas_sync_queue_schema() {
        let (db, _temp_dir) = create_test_db();

        // Verify cas_sync_queue table exists
        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='cas_sync_queue'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Verify status column has correct default and check constraint
        let status: String = db
            .conn
            .query_row(
                "SELECT dflt_value FROM pragma_table_info('cas_sync_queue') WHERE name='status'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "'pending'");
    }

    // CAS cache tests
}
