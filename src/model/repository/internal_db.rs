// DEPRECATED: The internal DB is deprecated and in the process of being removed.
// It has been superseded by use-case-specific databases.

use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use dirs;
use rusqlite::{Connection, params};
use std::collections::HashMap;
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

/// CAS sync queue record
#[derive(Debug, Clone)]
pub struct CasSyncRecord {
    pub id: i64,
    pub hash: String,
    pub data: String,
    pub metadata: HashMap<String, String>,
    pub attempts: u32,
}

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

    /// Enqueue a CAS object for syncing
    ///
    /// Takes raw JSON data, canonicalizes it (RFC 8785), computes SHA256 hash,
    /// and stores both in the queue.
    ///
    /// Returns the hash of the canonicalized content.
    pub fn enqueue_cas_object(
        &mut self,
        json_data: &serde_json::Value,
        metadata: Option<&HashMap<String, String>>,
    ) -> Result<String, GitAiError> {
        use sha2::{Digest, Sha256};

        // Canonicalize JSON (RFC 8785)
        let canonical = serde_json_canonicalizer::to_string(json_data)
            .map_err(|e| GitAiError::Generic(format!("Failed to canonicalize JSON: {}", e)))?;

        // Hash the canonicalized content
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let metadata_json = serde_json::to_string(metadata.unwrap_or(&HashMap::new()))?;

        self.conn.execute(
            r#"
            INSERT OR IGNORE INTO cas_sync_queue (
                hash, data, metadata, status, attempts, next_retry_at, created_at
            ) VALUES (?1, ?2, ?3, 'pending', 0, ?4, ?4)
            "#,
            params![hash, canonical, metadata_json, now],
        )?;

        Ok(hash)
    }

    /// Dequeue a batch of CAS objects for syncing (with lock acquisition)
    pub fn dequeue_cas_batch(
        &mut self,
        batch_size: usize,
    ) -> Result<Vec<CasSyncRecord>, GitAiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Step 1: Recover stale locks (processing for >10 minutes)
        let stale_threshold = now - 600; // 10 minutes
        self.conn.execute(
            r#"
            UPDATE cas_sync_queue
            SET status = 'pending', processing_started_at = NULL
            WHERE status = 'processing'
              AND processing_started_at < ?1
            "#,
            params![stale_threshold],
        )?;

        // Step 2: Atomically lock and fetch batch using UPDATE...RETURNING
        // Note: SQLite's UPDATE...RETURNING is atomic
        let mut stmt = self.conn.prepare(
            r#"
            UPDATE cas_sync_queue
            SET status = 'processing', processing_started_at = ?1
            WHERE id IN (
                SELECT id FROM cas_sync_queue
                WHERE status = 'pending'
                  AND next_retry_at <= ?2
                  AND attempts < 6
                ORDER BY next_retry_at
                LIMIT ?3
            )
            RETURNING id, hash, data, metadata, attempts
            "#,
        )?;

        let rows = stmt.query_map(params![now, now, batch_size], |row| {
            let metadata_json: String = row.get(3)?;
            let metadata: HashMap<String, String> =
                serde_json::from_str(&metadata_json).unwrap_or_default();
            let hash: String = row.get(1)?;
            let data: String = row.get(2)?;
            Ok(CasSyncRecord {
                id: row.get(0)?,
                hash,
                data,
                attempts: row.get(4)?,
                metadata,
            })
        })?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }

        Ok(records)
    }

    /// Delete a CAS sync record (on successful sync)
    pub fn delete_cas_sync_record(&mut self, id: i64) -> Result<(), GitAiError> {
        self.conn
            .execute("DELETE FROM cas_sync_queue WHERE id = ?", params![id])?;
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

    /// Get cached CAS messages by hash
    pub fn get_cas_cache(&self, hash: &str) -> Result<Option<String>, GitAiError> {
        let result = self.conn.query_row(
            "SELECT messages FROM cas_cache WHERE hash = ?1",
            params![hash],
            |row| row.get::<_, String>(0),
        );

        match result {
            Ok(messages) => Ok(Some(messages)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Cache CAS messages by hash (INSERT OR REPLACE since content is immutable)
    pub fn set_cas_cache(&mut self, hash: &str, messages_json: &str) -> Result<(), GitAiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        self.conn.execute(
            "INSERT OR REPLACE INTO cas_cache (hash, messages, cached_at) VALUES (?1, ?2, ?3)",
            params![hash, messages_json, now],
        )?;

        Ok(())
    }

    /// Update CAS sync record on failure (release lock, increment attempts, set next retry)
    pub fn update_cas_sync_failure(&mut self, id: i64, error: &str) -> Result<(), GitAiError> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Get current attempts count to calculate next retry
        let attempts: u32 = self.conn.query_row(
            "SELECT attempts FROM cas_sync_queue WHERE id = ?",
            params![id],
            |row| row.get(0),
        )?;

        let next_retry = calculate_next_retry(attempts + 1, now);

        self.conn.execute(
            r#"
            UPDATE cas_sync_queue
            SET status = 'pending',
                processing_started_at = NULL,
                attempts = attempts + 1,
                last_sync_error = ?1,
                last_sync_at = ?2,
                next_retry_at = ?3
            WHERE id = ?4
            "#,
            params![error, now, next_retry, id],
        )?;

        Ok(())
    }
}

/// Calculate next retry timestamp based on attempt number
fn calculate_next_retry(attempts: u32, now: i64) -> i64 {
    let delay_seconds = match attempts {
        1 => 5 * 60,       // 5 minutes
        2 => 30 * 60,      // 30 minutes
        3 => 2 * 60 * 60,  // 2 hours
        4 => 6 * 60 * 60,  // 6 hours
        5 => 12 * 60 * 60, // 12 hours
        _ => 24 * 60 * 60, // 24 hours (attempts >= 6)
    };
    now + delay_seconds
}

#[path = "internal_db_tests.rs"]
#[cfg(test)]
mod tests;
