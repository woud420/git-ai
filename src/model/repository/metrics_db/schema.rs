use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::MetricsDatabase;

/// Current schema version (must match MIGRATIONS.len())
pub(super) const SCHEMA_VERSION: usize = 5;

// This value is part of the metrics retry index schema. Changing it requires a
// migration that rebuilds `metrics_retryable` with the same literal used by
// the retry queries below; SQLite cannot prove a parameterized predicate
// implies a partial-index predicate.
pub(crate) const MAX_METRIC_UPLOAD_ATTEMPTS: u32 = 6;

/// Database migrations - each migration upgrades the schema by one version
pub(super) const MIGRATIONS: &[&str] = &[
    // Migration 0 -> 1: Initial schema with metrics table
    r#"
    CREATE TABLE IF NOT EXISTS metrics (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        event_json TEXT NOT NULL
    );
    "#,
    // Migration 1 -> 2: Persistent rate limiter state for agent_usage events
    r#"
    CREATE TABLE IF NOT EXISTS agent_usage_throttle (
        prompt_id TEXT PRIMARY KEY,
        last_sent_ts INTEGER NOT NULL
    );
    "#,
    // Migration 2 -> 3: Keep delivered metrics and add row-level retry state.
    r#"
    CREATE INDEX IF NOT EXISTS metrics_pending_retry
        ON metrics (delivered_ts, next_retry_at, id)
        WHERE delivered_ts IS NULL;

    CREATE INDEX IF NOT EXISTS metrics_processing_started_at
        ON metrics (processing_started_at)
        WHERE delivered_ts IS NULL AND processing_started_at IS NOT NULL;
    "#,
    // Migration 3 -> 4: Cache event metadata for efficient history/backfill queries.
    r#"
    CREATE INDEX IF NOT EXISTS metrics_event_ts_kind
        ON metrics (event_ts, event_kind, id)
        WHERE event_ts IS NOT NULL AND event_kind IS NOT NULL;

    CREATE INDEX IF NOT EXISTS metrics_session_kind_ts
        ON metrics (session_id, event_kind, event_ts, id)
        WHERE session_id IS NOT NULL
            AND event_kind IS NOT NULL
            AND event_ts IS NOT NULL;

    CREATE INDEX IF NOT EXISTS metrics_parent_session_kind_ts
        ON metrics (parent_session_id, event_kind, event_ts, id)
        WHERE parent_session_id IS NOT NULL
            AND event_kind IS NOT NULL
            AND event_ts IS NOT NULL;
    "#,
    // Migration 4 -> 5: Keep terminal history out of retry lookups. The
    // predicate and ordering intentionally match dequeue/count queries.
    r#"
    CREATE INDEX IF NOT EXISTS metrics_retryable
        ON metrics (next_retry_at ASC, id DESC)
        WHERE delivered_ts IS NULL
            AND processing_started_at IS NULL
            AND attempts < 6;

    DROP INDEX IF EXISTS metrics_pending_retry;
    "#,
];

/// Global database singleton
pub(super) static METRICS_DB: OnceLock<Mutex<MetricsDatabase>> = OnceLock::new();

impl MetricsDatabase {
    /// How long metric rows are retained for local history/offline retry (365 days).
    pub(super) const METRICS_RETENTION_SECS: u64 = 365 * 24 * 3600;
    /// Minimum interval between prune passes (24 hours).
    pub(super) const METRICS_PRUNE_INTERVAL_SECS: u64 = 24 * 3600;

    /// Get or initialize the global database
    pub fn global() -> Result<&'static Mutex<MetricsDatabase>, GitAiError> {
        let db_mutex = METRICS_DB.get_or_init(|| match Self::new() {
            Ok(db) => Mutex::new(db),
            Err(e) => {
                eprintln!("[Error] Failed to initialize metrics database: {}", e);
                Mutex::new(
                    Self::new_fallback().expect("Failed to create fallback metrics database"),
                )
            }
        });

        Ok(db_mutex)
    }

    /// Create a new database connection
    pub(super) fn new() -> Result<Self, GitAiError> {
        Self::open_at_path_impl(&Self::database_path()?)
    }

    pub(super) fn new_fallback() -> Result<Self, GitAiError> {
        let temp_path = std::env::temp_dir().join("git-ai-metrics-db-failed");
        Self::new_fallback_at_path(&temp_path)
    }

    pub(super) fn new_fallback_at_path(path: &std::path::Path) -> Result<Self, GitAiError> {
        Self::open_at_path_impl(path)
    }

    fn open_at_path_impl(path: &std::path::Path) -> Result<Self, GitAiError> {
        crate::model::repository::sqlite::open_at_path(path, |conn| {
            let mut db = Self { conn };
            db.initialize_schema()?;
            Ok(db)
        })
    }

    #[cfg(test)]
    pub(crate) fn new_temp_for_tests() -> Result<(Self, tempfile::TempDir), GitAiError> {
        let temp_dir = tempfile::TempDir::new()?;
        let db_path = temp_dir.path().join("metrics.db");
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path)?;
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            "#,
        )?;

        let mut db = Self { conn };
        db.initialize_schema()?;

        Ok((db, temp_dir))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn open_at_path(path: &std::path::Path) -> Result<Self, GitAiError> {
        Self::open_at_path_impl(path)
    }

    /// Get database path: ~/.git-ai/internal/metrics-db
    pub(super) fn database_path() -> Result<PathBuf, GitAiError> {
        // Allow test override via environment variable
        #[cfg(any(test, feature = "test-support"))]
        if let Ok(test_path) = std::env::var("GIT_AI_TEST_METRICS_DB_PATH") {
            return Ok(PathBuf::from(test_path));
        }

        let home = dirs::home_dir().ok_or_else(PersistenceError::home_dir_not_found)?;
        Ok(home.join(".git-ai").join("internal").join("metrics-db"))
    }

    /// Initialize schema and handle migrations
    pub(super) fn initialize_schema(&mut self) -> Result<(), GitAiError> {
        use crate::model::repository::sqlite;

        sqlite::ensure_schema_metadata_table(&self.conn)?;
        let current_version = sqlite::read_schema_version(&self.conn).unwrap_or(0);
        sqlite::migration_runner(
            &mut self.conn,
            "metrics",
            current_version,
            SCHEMA_VERSION,
            Self::apply_migration,
        )
    }

    /// Apply a single migration
    fn apply_migration(conn: &mut Connection, from_version: usize) -> Result<(), GitAiError> {
        if from_version >= MIGRATIONS.len() {
            return Err(PersistenceError::no_migration_path(
                "metrics",
                from_version,
                from_version + 1,
            ));
        }

        if from_version == 2 {
            add_row_level_retry_columns(conn)?;
        }
        if from_version == 3 {
            add_event_metadata_columns(conn)?;
        }

        let migration_sql = MIGRATIONS[from_version];
        let tx = conn.transaction()?;
        tx.execute_batch(migration_sql)?;
        tx.commit()?;

        Ok(())
    }
}

fn add_row_level_retry_columns(conn: &Connection) -> Result<(), GitAiError> {
    for (name, sql) in [
        (
            "delivered_ts",
            "ALTER TABLE metrics ADD COLUMN delivered_ts INTEGER",
        ),
        (
            "attempts",
            "ALTER TABLE metrics ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "last_sync_error",
            "ALTER TABLE metrics ADD COLUMN last_sync_error TEXT",
        ),
        (
            "last_sync_at",
            "ALTER TABLE metrics ADD COLUMN last_sync_at INTEGER",
        ),
        (
            "next_retry_at",
            "ALTER TABLE metrics ADD COLUMN next_retry_at INTEGER NOT NULL DEFAULT 0",
        ),
        (
            "processing_started_at",
            "ALTER TABLE metrics ADD COLUMN processing_started_at INTEGER",
        ),
    ] {
        add_column_if_missing(conn, "metrics", name, sql)?;
    }
    Ok(())
}

fn add_event_metadata_columns(conn: &Connection) -> Result<(), GitAiError> {
    for (name, sql) in [
        (
            "event_ts",
            "ALTER TABLE metrics ADD COLUMN event_ts INTEGER DEFAULT NULL",
        ),
        (
            "event_kind",
            "ALTER TABLE metrics ADD COLUMN event_kind INTEGER DEFAULT NULL",
        ),
        (
            "trace_id",
            "ALTER TABLE metrics ADD COLUMN trace_id TEXT DEFAULT NULL",
        ),
        (
            "session_id",
            "ALTER TABLE metrics ADD COLUMN session_id TEXT DEFAULT NULL",
        ),
        (
            "parent_session_id",
            "ALTER TABLE metrics ADD COLUMN parent_session_id TEXT DEFAULT NULL",
        ),
        (
            "tool",
            "ALTER TABLE metrics ADD COLUMN tool TEXT DEFAULT NULL",
        ),
        (
            "external_session_id",
            "ALTER TABLE metrics ADD COLUMN external_session_id TEXT DEFAULT NULL",
        ),
        (
            "external_parent_session_id",
            "ALTER TABLE metrics ADD COLUMN external_parent_session_id TEXT DEFAULT NULL",
        ),
        (
            "external_event_id",
            "ALTER TABLE metrics ADD COLUMN external_event_id TEXT DEFAULT NULL",
        ),
        (
            "external_parent_event_id",
            "ALTER TABLE metrics ADD COLUMN external_parent_event_id TEXT DEFAULT NULL",
        ),
        (
            "external_tool_use_id",
            "ALTER TABLE metrics ADD COLUMN external_tool_use_id TEXT DEFAULT NULL",
        ),
    ] {
        add_column_if_missing(conn, "metrics", name, sql)?;
    }
    Ok(())
}

pub(super) fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), GitAiError> {
    if column_exists(conn, table, column)? {
        return Ok(());
    }

    match conn.execute(alter_sql, []) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("duplicate column name") =>
        {
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

pub(super) fn column_exists(
    conn: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, GitAiError> {
    let count: i64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name = ?1"),
        params![column],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}
