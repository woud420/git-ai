//! Transcripts database for tracking stream cursors and watermarks.

use crate::model::stream_types::StreamError;
use crate::model::stream_types::StreamFormat;
use crate::model::stream_watermark::{WatermarkStrategy, WatermarkType};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Schema migrations - each entry is SQL to apply for that version.
const MIGRATIONS: &[&str] = &[
    // Version 1: Initial schema
    r#"
    CREATE TABLE IF NOT EXISTS schema_version (
        version INTEGER PRIMARY KEY
    );

    CREATE TABLE IF NOT EXISTS sessions (
        session_id TEXT PRIMARY KEY,
        agent_type TEXT NOT NULL,
        transcript_path TEXT NOT NULL,
        transcript_format TEXT NOT NULL,
        watermark_type TEXT NOT NULL,
        watermark_value TEXT NOT NULL,
        model TEXT,
        tool TEXT,
        external_thread_id TEXT,
        first_seen_at INTEGER NOT NULL,
        last_processed_at INTEGER NOT NULL,
        last_known_size INTEGER NOT NULL DEFAULT 0,
        last_modified INTEGER,
        processing_errors INTEGER DEFAULT 0,
        last_error TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_sessions_tool ON sessions(tool);
    CREATE INDEX IF NOT EXISTS idx_sessions_last_processed ON sessions(last_processed_at);
    CREATE INDEX IF NOT EXISTS idx_sessions_errors ON sessions(processing_errors) WHERE processing_errors > 0;
    CREATE INDEX IF NOT EXISTS idx_sessions_transcript_path ON sessions(transcript_path);

    CREATE TABLE IF NOT EXISTS processing_stats (
        session_id TEXT PRIMARY KEY,
        total_events INTEGER DEFAULT 0,
        total_bytes INTEGER DEFAULT 0,
        FOREIGN KEY (session_id) REFERENCES sessions(session_id)
    );

    INSERT INTO schema_version (version) VALUES (1);
    "#,
    // Version 2: Recreate sessions with external_session_id/external_parent_session_id,
    // drop model/tool columns and processing_stats table.
    // No data migration needed — transcripts feature has not shipped to production yet.
    r#"
    DROP TABLE IF EXISTS processing_stats;
    DROP TABLE IF EXISTS sessions;

    CREATE TABLE sessions (
        session_id TEXT PRIMARY KEY,
        tool TEXT NOT NULL,
        transcript_path TEXT NOT NULL,
        transcript_format TEXT NOT NULL,
        watermark_type TEXT NOT NULL,
        watermark_value TEXT NOT NULL,
        external_session_id TEXT NOT NULL,
        external_parent_session_id TEXT,
        first_seen_at INTEGER NOT NULL,
        last_processed_at INTEGER NOT NULL,
        last_known_size INTEGER NOT NULL DEFAULT 0,
        last_modified INTEGER,
        processing_errors INTEGER DEFAULT 0,
        last_error TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_sessions_tool ON sessions(tool);
    CREATE INDEX IF NOT EXISTS idx_sessions_last_processed ON sessions(last_processed_at);
    CREATE INDEX IF NOT EXISTS idx_sessions_errors ON sessions(processing_errors) WHERE processing_errors > 0;
    CREATE INDEX IF NOT EXISTS idx_sessions_transcript_path ON sessions(transcript_path);

    INSERT INTO schema_version (version) VALUES (2);
    "#,
    // Version 3: Add repo_work_dir column for session-level repo context.
    r#"
    ALTER TABLE sessions ADD COLUMN repo_work_dir TEXT;

    INSERT INTO schema_version (version) VALUES (3);
    "#,
    // Version 4: Add stream_kind column with compound PK (session_id, stream_kind, stream_path).
    // The path is part of the PK to prevent collisions when two physically distinct files
    // produce the same session_id (issue #1461).
    r#"
    BEGIN;
    CREATE TABLE tracked_streams_v4 (
        session_id TEXT NOT NULL,
        stream_kind TEXT NOT NULL DEFAULT 'transcript',
        tool TEXT NOT NULL,
        stream_path TEXT NOT NULL,
        stream_format TEXT NOT NULL,
        watermark_type TEXT NOT NULL,
        watermark_value TEXT NOT NULL,
        external_session_id TEXT NOT NULL,
        external_parent_session_id TEXT,
        first_seen_at INTEGER NOT NULL,
        last_processed_at INTEGER NOT NULL,
        last_known_size INTEGER NOT NULL DEFAULT 0,
        last_modified INTEGER,
        processing_errors INTEGER DEFAULT 0,
        last_error TEXT,
        repo_work_dir TEXT,
        PRIMARY KEY (session_id, stream_kind, stream_path)
    );
    INSERT INTO tracked_streams_v4 SELECT session_id, 'transcript', tool, transcript_path, transcript_format, watermark_type, watermark_value, external_session_id, external_parent_session_id, first_seen_at, last_processed_at, last_known_size, last_modified, processing_errors, last_error, repo_work_dir FROM sessions;
    DROP TABLE sessions;
    ALTER TABLE tracked_streams_v4 RENAME TO tracked_streams;
    CREATE INDEX idx_streams_tool ON tracked_streams(tool);
    CREATE INDEX idx_streams_last_processed ON tracked_streams(last_processed_at);
    CREATE INDEX idx_streams_errors ON tracked_streams(processing_errors) WHERE processing_errors > 0;
    CREATE INDEX idx_streams_stream_path ON tracked_streams(stream_path);
    INSERT INTO schema_version (version) VALUES (4);
    COMMIT;
    "#,
];

/// Record representing a tracked stream cursor in the database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamRecord {
    pub session_id: String,
    pub stream_kind: String,
    pub tool: String,
    pub stream_path: String,
    pub stream_format: StreamFormat,
    pub watermark_type: WatermarkType,
    pub watermark_value: String,
    pub external_session_id: String,
    pub external_parent_session_id: Option<String>,
    pub first_seen_at: i64,
    pub last_processed_at: i64,
    pub last_known_size: i64,
    pub last_modified: Option<i64>,
    pub processing_errors: i64,
    pub last_error: Option<String>,
    pub repo_work_dir: Option<String>,
}

/// SQLite database for transcript tracking.
pub struct StreamsDatabase {
    conn: Arc<Mutex<Connection>>,
}

impl StreamsDatabase {
    /// Open or create the transcripts database at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StreamError> {
        let conn = crate::model::repository::sqlite::open_with_memory_limits(path.as_ref())
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to open database: {}", e),
            })?;

        // Enable WAL mode for better concurrency and crash resistance
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to enable WAL mode: {}", e),
            })?;

        // Performance optimizations
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to set synchronous mode: {}", e),
            })?;
        conn.pragma_update(None, "temp_store", "MEMORY")
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to set temp store: {}", e),
            })?;

        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };

        // Run migrations
        db.migrate()?;

        Ok(db)
    }

    /// Run database migrations to bring schema up to current version.
    fn migrate(&self) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // Check if schema_version table exists
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='schema_version'",
                [],
                |row| {
                    let count: i64 = row.get(0)?;
                    Ok(count > 0)
                },
            )
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to check schema_version table: {}", e),
            })?;

        // Get current schema version (0 if table doesn't exist)
        let current_version: u32 = if table_exists {
            conn.query_row(
                "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to query schema version: {}", e),
            })?
            .unwrap_or(0)
        } else {
            0
        };

        // Apply migrations
        for (version, migration_sql) in MIGRATIONS.iter().enumerate() {
            let target_version = (version + 1) as u32;
            if current_version < target_version {
                conn.execute_batch(migration_sql)
                    .map_err(|e| StreamError::Fatal {
                        message: format!(
                            "Failed to apply migration to version {}: {}",
                            target_version, e
                        ),
                    })?;
            }
        }

        Ok(())
    }

    /// Insert a new stream record.
    pub fn insert_stream(&self, record: &StreamRecord) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        conn.execute(
            r#"
            INSERT INTO tracked_streams (
                session_id, stream_kind, tool, stream_path, stream_format,
                watermark_type, watermark_value, external_session_id,
                external_parent_session_id,
                first_seen_at, last_processed_at, last_known_size, last_modified,
                processing_errors, last_error, repo_work_dir
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
            params![
                record.session_id,
                record.stream_kind,
                record.tool,
                record.stream_path,
                record.stream_format.to_string(),
                record.watermark_type.to_string(),
                record.watermark_value,
                record.external_session_id,
                record.external_parent_session_id,
                record.first_seen_at,
                record.last_processed_at,
                record.last_known_size,
                record.last_modified,
                record.processing_errors,
                record.last_error,
                record.repo_work_dir,
            ],
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to insert stream: {}", e),
        })?;

        Ok(())
    }

    /// Helper to map a row to a StreamRecord. Returns an error if any enum field is unrecognized.
    fn row_to_stream(row: &rusqlite::Row) -> rusqlite::Result<StreamRecord> {
        let fmt_str: String = row.get(4)?;
        let wm_str: String = row.get(5)?;
        let stream_format = fmt_str.parse().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let watermark_type = wm_str.parse().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?;
        Ok(StreamRecord {
            session_id: row.get(0)?,
            stream_kind: row.get(1)?,
            tool: row.get(2)?,
            stream_path: row.get(3)?,
            stream_format,
            watermark_type,
            watermark_value: row.get(6)?,
            external_session_id: row.get(7)?,
            external_parent_session_id: row.get(8)?,
            first_seen_at: row.get(9)?,
            last_processed_at: row.get(10)?,
            last_known_size: row.get(11)?,
            last_modified: row.get(12)?,
            processing_errors: row.get(13)?,
            last_error: row.get(14)?,
            repo_work_dir: row.get(15)?,
        })
    }

    /// Get a stream record by its full primary key.
    pub fn get_stream(
        &self,
        session_id: &str,
        stream_kind: &str,
        stream_path: &str,
    ) -> Result<Option<StreamRecord>, StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        conn.query_row(
            r#"
            SELECT session_id, stream_kind, tool, stream_path, stream_format,
                   watermark_type, watermark_value, external_session_id,
                   external_parent_session_id,
                   first_seen_at, last_processed_at, last_known_size, last_modified,
                   processing_errors, last_error, repo_work_dir
            FROM tracked_streams WHERE session_id = ?1 AND stream_kind = ?2 AND stream_path = ?3
            "#,
            params![session_id, stream_kind, stream_path],
            Self::row_to_stream,
        )
        .optional()
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to get stream: {}", e),
        })
    }

    /// Update the watermark for a stream.
    pub fn update_watermark(
        &self,
        session_id: &str,
        stream_kind: &str,
        stream_path: &str,
        watermark: &dyn WatermarkStrategy,
    ) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Utc::now().timestamp();
        let watermark_value = watermark.serialize();

        let rows_changed = conn.execute(
            "UPDATE tracked_streams SET watermark_value = ?1, last_processed_at = ?2 WHERE session_id = ?3 AND stream_kind = ?4 AND stream_path = ?5",
            params![watermark_value, now, session_id, stream_kind, stream_path],
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to update watermark: {}", e),
        })?;

        if rows_changed == 0 {
            return Err(StreamError::Fatal {
                message: format!("Stream not found: {}", session_id),
            });
        }

        Ok(())
    }

    /// Update file metadata (size and modified time) for a stream.
    pub fn update_file_metadata(
        &self,
        session_id: &str,
        stream_kind: &str,
        stream_path: &str,
        file_size: u64,
        modified: Option<DateTime<Utc>>,
    ) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let modified_ts = modified.map(|dt| dt.timestamp());

        let rows_changed = conn.execute(
            "UPDATE tracked_streams SET last_known_size = ?1, last_modified = ?2 WHERE session_id = ?3 AND stream_kind = ?4 AND stream_path = ?5",
            params![file_size as i64, modified_ts, session_id, stream_kind, stream_path],
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to update file metadata: {}", e),
        })?;

        if rows_changed == 0 {
            return Err(StreamError::Fatal {
                message: format!("Stream not found: {}", session_id),
            });
        }

        Ok(())
    }

    /// Update the repo_work_dir for a stream.
    pub fn update_repo_work_dir(
        &self,
        session_id: &str,
        stream_kind: &str,
        stream_path: &str,
        repo_work_dir: &str,
    ) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let rows_changed = conn
            .execute(
                "UPDATE tracked_streams SET repo_work_dir = ?1 WHERE session_id = ?2 AND stream_kind = ?3 AND stream_path = ?4",
                params![repo_work_dir, session_id, stream_kind, stream_path],
            )
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to update repo_work_dir: {}", e),
            })?;

        if rows_changed == 0 {
            return Err(StreamError::Fatal {
                message: format!("Stream not found: {}", session_id),
            });
        }

        Ok(())
    }

    /// Record an error for a stream.
    pub fn record_error(
        &self,
        session_id: &str,
        stream_kind: &str,
        stream_path: &str,
        error_message: &str,
    ) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let rows_changed = conn.execute(
            "UPDATE tracked_streams SET processing_errors = processing_errors + 1, last_error = ?1 WHERE session_id = ?2 AND stream_kind = ?3 AND stream_path = ?4",
            params![error_message, session_id, stream_kind, stream_path],
        )
        .map_err(|e| StreamError::Fatal {
            message: format!("Failed to record error: {}", e),
        })?;

        if rows_changed == 0 {
            return Err(StreamError::Fatal {
                message: format!("Stream not found: {}", session_id),
            });
        }

        Ok(())
    }

    /// Delete a stream and its associated data.
    pub fn delete_stream(
        &self,
        session_id: &str,
        stream_kind: &str,
        stream_path: &str,
    ) -> Result<(), StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let rows_changed = conn
            .execute(
                "DELETE FROM tracked_streams WHERE session_id = ?1 AND stream_kind = ?2 AND stream_path = ?3",
                params![session_id, stream_kind, stream_path],
            )
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to delete stream: {}", e),
            })?;

        if rows_changed == 0 {
            return Err(StreamError::Fatal {
                message: format!("Stream not found: {}", session_id),
            });
        }

        Ok(())
    }

    /// Get all stream records.
    pub fn all_streams(&self) -> Result<Vec<StreamRecord>, StreamError> {
        let conn = self
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut stmt = conn
            .prepare(
                r#"
            SELECT session_id, stream_kind, tool, stream_path, stream_format,
                   watermark_type, watermark_value, external_session_id,
                   external_parent_session_id,
                   first_seen_at, last_processed_at, last_known_size, last_modified,
                   processing_errors, last_error, repo_work_dir
            FROM tracked_streams
            "#,
            )
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to prepare all_streams query: {}", e),
            })?;

        let rows = stmt
            .query_map([], Self::row_to_stream)
            .map_err(|e| StreamError::Fatal {
                message: format!("Failed to query all streams: {}", e),
            })?;

        let mut streams = Vec::new();
        for row in rows {
            match row {
                Ok(record) => streams.push(record),
                Err(e) => {
                    tracing::warn!(error = %e, "Skipping stream row with unrecognized format or watermark type")
                }
            }
        }

        Ok(streams)
    }
}

#[cfg(test)]
mod tests_records;
#[cfg(test)]
mod tests_resilience;
