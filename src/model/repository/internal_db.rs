// DEPRECATED: The internal DB is deprecated and in the process of being removed.
// It has been superseded by use-case-specific databases.

use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use dirs;
use rusqlite::{Connection, params};
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

        // FAST PATH: Check if database is already at current version
        // This avoids expensive schema operations on every process start
        if let Some(current_version) = sqlite::read_schema_version(&self.conn) {
            if current_version == SCHEMA_VERSION {
                // Database is up-to-date, no migrations needed
                return Ok(());
            }
            if current_version > SCHEMA_VERSION {
                // Forward-compatible: an older binary can still read/write
                // known tables even if a newer binary added extra tables.
                // Just skip migrations and use what we have.
                return Ok(());
            }
            // Fall through to apply missing migrations (current_version < SCHEMA_VERSION)
        }
        // If the read found nothing, the table doesn't exist - proceed with full
        // initialization.

        // Step 1: Create schema_metadata table (this is the only table we create directly)
        sqlite::ensure_schema_metadata_table(&self.conn)?;

        // Step 2: Get current schema version (0 if brand new database)
        let current_version = sqlite::read_schema_version(&self.conn).unwrap_or(0);

        // Step 3: Apply all missing migrations sequentially
        for target_version in current_version..SCHEMA_VERSION {
            tracing::debug!(
                "[Migration] Upgrading database from version {} to {}",
                target_version,
                target_version + 1
            );

            // Apply the migration (FATAL on error)
            self.apply_migration(target_version)?;

            // Use an upsert so concurrent initializers do not race on version row creation.
            self.conn.execute(
                r#"
                INSERT INTO schema_metadata (key, value)
                VALUES ('version', ?1)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value
                WHERE CAST(schema_metadata.value AS INTEGER) < CAST(excluded.value AS INTEGER)
                "#,
                params![(target_version + 1).to_string()],
            )?;

            tracing::debug!(
                "[Migration] Successfully upgraded to version {}",
                target_version + 1
            );
        }

        // Step 5: Verify final version matches expected
        let final_version: usize = self.conn.query_row(
            "SELECT value FROM schema_metadata WHERE key = 'version'",
            [],
            |row| {
                let version_str: String = row.get(0)?;
                version_str
                    .parse::<usize>()
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
            },
        )?;

        if final_version != SCHEMA_VERSION {
            return Err(GitAiError::Generic(format!(
                "Migration failed: expected version {} but got version {}",
                SCHEMA_VERSION, final_version
            )));
        }

        Ok(())
    }

    /// Apply a single migration
    /// Migration failures are FATAL - the program cannot continue with a partially migrated database
    fn apply_migration(&mut self, from_version: usize) -> Result<(), GitAiError> {
        if from_version >= MIGRATIONS.len() {
            return Err(PersistenceError::no_migration_path(
                "internal",
                from_version,
                from_version + 1,
            ));
        }

        let migration_sql = MIGRATIONS[from_version];

        // Execute migration in a transaction for atomicity
        let tx = self.conn.transaction()?;
        tx.execute_batch(migration_sql)?;
        tx.commit()?;

        Ok(())
    }

    /// Delete CAS sync records by their content hashes (used by daemon after successful upload).
    pub fn delete_cas_by_hashes(&mut self, hashes: &[String]) -> Result<usize, GitAiError> {
        if hashes.is_empty() {
            return Ok(0);
        }
        let placeholders: Vec<&str> = hashes.iter().map(|_| "?").collect();
        let sql = format!(
            "DELETE FROM cas_sync_queue WHERE hash IN ({})",
            placeholders.join(",")
        );
        let params: Vec<&dyn rusqlite::ToSql> =
            hashes.iter().map(|h| h as &dyn rusqlite::ToSql).collect();
        let deleted = self.conn.execute(&sql, params.as_slice())?;
        Ok(deleted)
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
