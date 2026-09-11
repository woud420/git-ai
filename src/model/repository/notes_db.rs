//! Dedicated notes-backend storage at `~/.git-ai/internal/notes-db`.
//!
//! Single `notes` table that serves three roles, distinguished by `origin`:
//!  - `origin = 'local'` — primary storage for the sqlite notes backend; these
//!    rows are never uploaded and never evicted
//!  - `origin = 'queue'` — HTTP-backend rows: `synced = 0` pending upload,
//!    `synced = 1` uploaded (kept as read cache)
//!  - `origin = 'cache'` — read-cache rows imported from the remote backend or
//!    from `refs/notes/ai`; evictable
//!
//! Rows are NEVER deleted on successful upload — they are retained as the local
//! read cache so that subsequent reads can be served without git or a network call.
//!
//! This database is SEPARATE from `src/authorship/internal_db.rs`. Adding columns
//! or migrations to `internal_db` for this feature is explicitly not what we do here.

use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use rusqlite::{Connection, ToSql, params, params_from_iter};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Current schema version (must equal MIGRATIONS.len()).
const SCHEMA_VERSION: usize = 2;

/// Database migrations — each entry upgrades the schema by one version.
const MIGRATIONS: &[&str] = &[
    // Migration 0 → 1: single notes table with synced flag
    r#"
    CREATE TABLE IF NOT EXISTS notes (
        commit_sha              TEXT PRIMARY KEY NOT NULL,
        content                 TEXT NOT NULL,
        synced                  INTEGER NOT NULL DEFAULT 0,
        attempts                INTEGER NOT NULL DEFAULT 0,
        last_sync_error         TEXT,
        last_sync_at            INTEGER,
        next_retry_at           INTEGER NOT NULL DEFAULT 0,
        processing_started_at   INTEGER,
        created_at              INTEGER NOT NULL,
        updated_at              INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_notes_pending
        ON notes(synced, next_retry_at) WHERE synced = 0;
    "#,
    // Migration 1 → 2: origin column distinguishing local-primary rows
    // (sqlite backend), HTTP queue rows, and evictable cache rows. Existing
    // rows were all written by the HTTP queue path.
    r#"
    ALTER TABLE notes ADD COLUMN origin TEXT NOT NULL DEFAULT 'queue';
    "#,
];

/// Shared upsert SQL for the queue-backend note path (single writes and
/// batches): preserves `synced`/`attempts`/`next_retry_at` when content is
/// unchanged, otherwise resets them so the updated note is re-queued for
/// upload.
const UPSERT_NOTE_SQL: &str = r#"
INSERT INTO notes (commit_sha, content, synced, created_at, updated_at, next_retry_at)
VALUES (?1, ?2, 0, ?3, ?3, ?3)
ON CONFLICT(commit_sha) DO UPDATE SET
    content        = excluded.content,
    synced         = CASE WHEN notes.content = excluded.content THEN notes.synced ELSE 0 END,
    attempts       = CASE WHEN notes.content = excluded.content THEN notes.attempts ELSE 0 END,
    next_retry_at  = CASE WHEN notes.content = excluded.content THEN notes.next_retry_at ELSE excluded.next_retry_at END,
    updated_at     = excluded.updated_at
"#;

/// Global singleton for the notes database.
static NOTES_DB: OnceLock<Mutex<NotesDatabase>> = OnceLock::new();

/// A pending note returned from `dequeue_pending`.
#[derive(Debug, Clone)]
pub struct PendingNote {
    pub commit_sha: String,
    pub content: String,
    pub attempts: i64,
}

/// SQLite wrapper for notes storage and queue.
pub struct NotesDatabase {
    conn: Connection,
}

impl NotesDatabase {
    /// Return (or lazily initialize) the global database mutex.
    ///
    /// In test builds with `GIT_AI_TEST_NOTES_DB_PATH` set, the OnceLock singleton
    /// cannot be re-initialized per-test. Tests requiring isolated DB instances should
    /// use `open_at_path()` directly instead of relying on this singleton.
    pub fn global() -> Result<&'static Mutex<NotesDatabase>, GitAiError> {
        let db_mutex = NOTES_DB.get_or_init(|| match Self::new() {
            Ok(db) => Mutex::new(db),
            Err(e) => {
                eprintln!("[Error] Failed to initialize notes database: {}", e);
                // Fall back to a temp file so the process can continue running.
                let temp_path = std::env::temp_dir().join("git-ai-notes-db-failed");
                let conn = crate::model::repository::sqlite::open_with_memory_limits(&temp_path)
                    .expect("Failed to create temp DB");
                Mutex::new(NotesDatabase { conn })
            }
        });
        Ok(db_mutex)
    }

    /// Open a database at an explicit path. Useful for tests that need an isolated
    /// DB instance without relying on the process-global OnceLock singleton.
    pub fn open_at_path(path: &std::path::Path) -> Result<Self, GitAiError> {
        crate::model::repository::sqlite::open_at_path(path, |conn| {
            let mut db = Self { conn };
            db.initialize_schema()?;
            Ok(db)
        })
    }

    /// Open (or create) the database at the configured path.
    fn new() -> Result<Self, GitAiError> {
        Self::open_at_path(&Self::database_path()?)
    }

    /// Resolve the on-disk path for the notes database.
    ///
    /// In tests, the `GIT_AI_TEST_NOTES_DB_PATH` environment variable overrides
    /// the default location so that test runs are isolated.
    fn database_path() -> Result<PathBuf, GitAiError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Ok(test_path) = std::env::var("GIT_AI_TEST_NOTES_DB_PATH") {
            return Ok(PathBuf::from(test_path));
        }

        let home = dirs::home_dir().ok_or_else(PersistenceError::home_dir_not_found)?;
        Ok(home.join(".git-ai").join("internal").join("notes-db"))
    }

    /// Apply schema migrations until the DB is at `SCHEMA_VERSION`.
    fn initialize_schema(&mut self) -> Result<(), GitAiError> {
        use crate::model::repository::sqlite;

        sqlite::ensure_schema_metadata_table(&self.conn)?;
        let current_version = sqlite::read_schema_version(&self.conn).unwrap_or(0);
        sqlite::migration_runner(
            &mut self.conn,
            "notes",
            current_version,
            SCHEMA_VERSION,
            Self::apply_migration,
        )
    }

    fn apply_migration(conn: &mut Connection, from_version: usize) -> Result<(), GitAiError> {
        if from_version >= MIGRATIONS.len() {
            return Err(PersistenceError::no_migration_path(
                "notes",
                from_version,
                from_version + 1,
            ));
        }

        let migration_sql = MIGRATIONS[from_version];
        let tx = conn.transaction()?;
        tx.execute_batch(migration_sql)?;
        tx.commit()?;

        Ok(())
    }

    // ----- Write operations -----

    /// Upsert a note.
    ///
    /// - New rows are inserted with `synced = 0` (pending upload).
    /// - If the row already exists with *the same content*, the `synced` flag and
    ///   attempt count are preserved.
    /// - If the content changed, `synced` and `attempts` are reset to 0 so the
    ///   updated note is queued for re-upload.
    pub fn upsert_note(&mut self, commit_sha: &str, content: &str) -> Result<(), GitAiError> {
        let now = unix_now();
        self.conn
            .execute(UPSERT_NOTE_SQL, params![commit_sha, content, now])?;
        Ok(())
    }

    /// Upsert a batch of notes inside a single transaction.
    pub fn upsert_notes_batch(&mut self, entries: &[(String, String)]) -> Result<(), GitAiError> {
        self.batch_upsert(entries, UPSERT_NOTE_SQL)
    }

    /// Upsert a note as sqlite-backend primary storage (`origin = 'local'`).
    ///
    /// Local-primary rows are terminal: never enqueued for upload and never
    /// evicted by cache maintenance.
    pub fn upsert_local_note(&mut self, commit_sha: &str, content: &str) -> Result<(), GitAiError> {
        self.upsert_local_notes_batch(&[(commit_sha.to_string(), content.to_string())])
    }

    /// Upsert a batch of local-primary notes inside a single transaction.
    pub fn upsert_local_notes_batch(
        &mut self,
        entries: &[(String, String)],
    ) -> Result<(), GitAiError> {
        self.batch_upsert(
            entries,
            r#"
            INSERT INTO notes (commit_sha, content, synced, origin, created_at, updated_at, next_retry_at)
            VALUES (?1, ?2, 1, 'local', ?3, ?3, 0)
            ON CONFLICT(commit_sha) DO UPDATE SET
                content    = excluded.content,
                origin     = 'local',
                synced     = 1,
                updated_at = excluded.updated_at
            "#,
        )
    }

    /// Read all local-primary notes (used by `git-ai notes migrate --to git-notes`).
    pub fn get_local_notes(&self) -> Result<Vec<(String, String)>, GitAiError> {
        let mut stmt = self
            .conn
            .prepare("SELECT commit_sha, content FROM notes WHERE origin = 'local'")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Insert already-synced rows (used for cache-warming on pull and migration).
    ///
    /// Rows are inserted with `synced = 1` and `origin = 'cache'`; they act as
    /// read cache but are not enqueued for upload. Local-primary rows are never
    /// overwritten by cache imports.
    pub fn cache_synced_notes(&mut self, entries: &[(String, String)]) -> Result<(), GitAiError> {
        self.batch_upsert(
            entries,
            r#"
            INSERT INTO notes (commit_sha, content, synced, origin, created_at, updated_at, last_sync_at, next_retry_at)
            VALUES (?1, ?2, 1, 'cache', ?3, ?3, ?3, 0)
            ON CONFLICT(commit_sha) DO UPDATE SET
                content      = excluded.content,
                synced       = 1,
                last_sync_at = excluded.last_sync_at,
                updated_at   = excluded.updated_at
            WHERE notes.origin != 'local'
            "#,
        )
    }

    /// Run `sql` as an `INSERT ... ON CONFLICT` upsert for each `(commit_sha,
    /// content)` entry inside a single transaction, binding `(sha, content,
    /// now)` as `?1, ?2, ?3`. Shared skeleton for the three note-upsert-batch
    /// variants above, which differ only in the SQL (and thus which columns/
    /// origin get written).
    fn batch_upsert(&mut self, entries: &[(String, String)], sql: &str) -> Result<(), GitAiError> {
        if entries.is_empty() {
            return Ok(());
        }
        let now = unix_now();
        let tx = self.conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(sql)?;
            for (sha, content) in entries {
                stmt.execute(params![sha, content, now])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ----- Queue operations -----

    /// Lock and return a batch of pending notes for upload.
    ///
    /// Sets `processing_started_at` on selected rows so concurrent workers do not
    /// pick up the same rows. Stale locks (older than 10 minutes) are automatically
    /// released before the batch is selected.
    ///
    /// Rows with `attempts >= 6` are skipped (permanent failure backoff).
    pub fn dequeue_pending(&mut self, batch_size: usize) -> Result<Vec<PendingNote>, GitAiError> {
        let now = unix_now();
        let stale_cutoff = now - 600; // 10 minutes

        // Release stale processing locks so they can be retried.
        self.conn.execute(
            r#"UPDATE notes
               SET processing_started_at = NULL
               WHERE synced = 0
                 AND processing_started_at IS NOT NULL
                 AND processing_started_at < ?1"#,
            params![stale_cutoff],
        )?;

        // Select eligible rows first, then lock them. Two-step approach avoids
        // UPDATE ... RETURNING which requires SQLite 3.35+.
        let shas: Vec<String> = {
            let mut stmt = self.conn.prepare(
                r#"SELECT commit_sha FROM notes
                   WHERE synced = 0
                     AND origin = 'queue'
                     AND processing_started_at IS NULL
                     AND next_retry_at <= ?1
                     AND attempts < 6
                   ORDER BY next_retry_at
                   LIMIT ?2"#,
            )?;
            let rows = stmt.query_map(params![now, batch_size as i64], |row| {
                row.get::<_, String>(0)
            })?;
            rows.filter_map(|r| r.ok()).collect()
        };

        if shas.is_empty() {
            return Ok(Vec::new());
        }

        // Lock the selected rows.
        let placeholders = numbered_placeholders(shas.len(), 2);
        let update_sql = format!(
            "UPDATE notes SET processing_started_at = ?1 WHERE commit_sha IN ({})",
            placeholders
        );
        let params =
            std::iter::once(&now as &dyn ToSql).chain(shas.iter().map(|sha| sha as &dyn ToSql));
        self.conn.execute(&update_sql, params_from_iter(params))?;

        // Read back the locked rows.
        let select_sql = format!(
            "SELECT commit_sha, content, attempts FROM notes WHERE commit_sha IN ({})",
            numbered_placeholders(shas.len(), 1)
        );
        let mut stmt = self.conn.prepare(&select_sql)?;
        let rows = stmt.query_map(params_from_iter(shas.iter()), |row| {
            Ok(PendingNote {
                commit_sha: row.get(0)?,
                content: row.get(1)?,
                attempts: row.get(2)?,
            })
        })?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Mark a set of notes as successfully synced.
    ///
    /// Sets `synced = 1` and clears `processing_started_at`. Returns the number
    /// of rows updated.
    pub fn mark_synced(&mut self, commit_shas: &[String]) -> Result<usize, GitAiError> {
        if commit_shas.is_empty() {
            return Ok(0);
        }
        let now = unix_now();

        // Build a parameterised `IN (...)` clause.
        let placeholders = numbered_placeholders(commit_shas.len(), 2);
        let sql = format!(
            "UPDATE notes SET synced = 1, last_sync_at = ?1, processing_started_at = NULL \
             WHERE commit_sha IN ({})",
            placeholders
        );

        let params = std::iter::once(&now as &dyn ToSql)
            .chain(commit_shas.iter().map(|sha| sha as &dyn ToSql));
        let updated = self.conn.execute(&sql, params_from_iter(params))?;
        Ok(updated)
    }

    /// Mark a batch as failed: release the lock, increment attempts, and schedule
    /// exponential-backoff retry.
    pub fn mark_failed(&mut self, commit_shas: &[String], error: &str) -> Result<(), GitAiError> {
        if commit_shas.is_empty() {
            return Ok(());
        }
        let now = unix_now();
        let tx = self.conn.transaction()?;
        for sha in commit_shas {
            tx.execute(
                r#"UPDATE notes
                   SET processing_started_at = NULL,
                       attempts              = attempts + 1,
                       last_sync_error       = ?1,
                       last_sync_at          = ?2,
                       next_retry_at         = ?2 + (1 << MIN(attempts + 1, 8)) * 5
                   WHERE commit_sha = ?3 AND synced = 0"#,
                params![error, now, sha],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    // ----- Read operations -----

    /// Count unsynced notes that can be dequeued for upload right now.
    pub fn count_pending_uploadable(&self) -> Result<usize, GitAiError> {
        let now = unix_now();
        let count: i64 = self.conn.query_row(
            r#"SELECT COUNT(*) FROM notes
               WHERE synced = 0
                 AND processing_started_at IS NULL
                 AND next_retry_at <= ?1
                 AND attempts < 6"#,
            params![now],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Retrieve the note content for a single commit SHA.
    pub fn get_note(&self, commit_sha: &str) -> Result<Option<String>, GitAiError> {
        match self.conn.query_row(
            "SELECT content FROM notes WHERE commit_sha = ?1",
            params![commit_sha],
            |row| row.get::<_, String>(0),
        ) {
            Ok(c) => Ok(Some(c)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Return the subset of `commit_shas` that exist in the DB with `synced = 1`.
    pub fn get_synced_shas(&self, commit_shas: &[&str]) -> Result<HashSet<String>, GitAiError> {
        if commit_shas.is_empty() {
            return Ok(HashSet::new());
        }
        let placeholders = numbered_placeholders(commit_shas.len(), 1);
        let sql = format!(
            "SELECT commit_sha FROM notes WHERE synced = 1 AND commit_sha IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(commit_shas.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        let mut result = HashSet::new();
        for row in rows {
            result.insert(row?);
        }
        Ok(result)
    }

    /// Retrieve note content for a slice of commit SHAs.
    ///
    /// Only SHAs that exist in the database are returned; missing SHAs are absent
    /// from the result map.
    pub fn get_notes(&self, commit_shas: &[&str]) -> Result<HashMap<String, String>, GitAiError> {
        if commit_shas.is_empty() {
            return Ok(HashMap::new());
        }
        let placeholders = numbered_placeholders(commit_shas.len(), 1);
        let sql = format!(
            "SELECT commit_sha, content FROM notes WHERE commit_sha IN ({})",
            placeholders
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(commit_shas.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut result = HashMap::new();
        for row in rows {
            let (sha, content) = row?;
            result.insert(sha, content);
        }
        Ok(result)
    }

    /// Return the commit SHAs of notes whose content contains `needle` as a
    /// literal substring (LIKE wildcards in the needle are escaped).
    pub fn search_notes_content(&self, needle: &str) -> Result<Vec<String>, GitAiError> {
        let escaped = needle
            .replace('\\', r"\\")
            .replace('%', r"\%")
            .replace('_', r"\_");
        let pattern = format!("%{}%", escaped);

        let mut stmt = self
            .conn
            .prepare(r"SELECT commit_sha FROM notes WHERE content LIKE ?1 ESCAPE '\'")?;
        let rows = stmt.query_map(params![pattern], |row| row.get::<_, String>(0))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Evict synced cache entries older than `max_age_secs` when the total row
    /// count exceeds `max_rows`. Returns the number of rows deleted.
    pub fn evict_stale_cache(
        &mut self,
        max_rows: usize,
        max_age_secs: i64,
    ) -> Result<usize, GitAiError> {
        // Local-primary rows (sqlite backend) are never counted or evicted.
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM notes WHERE synced = 1 AND origin != 'local'",
            [],
            |row| row.get(0),
        )?;
        if (count as usize) <= max_rows {
            return Ok(0);
        }
        let cutoff = unix_now() - max_age_secs;
        let deleted = self.conn.execute(
            "DELETE FROM notes WHERE synced = 1 AND origin != 'local' AND last_sync_at < ?1",
            params![cutoff],
        )?;
        Ok(deleted)
    }
}

fn numbered_placeholders(count: usize, first: usize) -> String {
    (0..count)
        .map(|i| format!("?{}", i + first))
        .collect::<Vec<_>>()
        .join(",")
}

fn unix_now() -> i64 {
    crate::model::clock::now_secs() as i64
}

#[path = "notes_db_tests.rs"]
#[cfg(test)]
mod tests;
