mod schema;
use schema::MIGRATIONS;

use crate::error::GitAiError;
use crate::model::repository::error::PersistenceError;
use crate::model::working_log::AgentId;
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

const SCHEMA_VERSION: usize = 2;
const RETENTION_SECS: u64 = 30 * 24 * 3600;
const PRUNE_INTERVAL_SECS: u64 = 24 * 3600;

static BASH_HISTORY_DB: OnceLock<Result<Mutex<BashHistoryDatabase>, String>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct BashCallStart {
    pub original_cwd: String,
    pub repo_work_dir: Option<String>,
    pub repo_discovery_error: Option<String>,
    pub session_id: String,
    pub tool_use_id: String,
    pub agent_id: AgentId,
    pub start_trace_id: String,
    pub started_at_ns: u128,
    pub command: Option<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct BashCallEnd {
    pub original_cwd: String,
    pub repo_work_dir: Option<String>,
    pub repo_discovery_error: Option<String>,
    pub session_id: String,
    pub tool_use_id: String,
    pub agent_id: AgentId,
    pub start_trace_id: Option<String>,
    pub end_trace_id: String,
    pub started_at_ns: Option<u128>,
    pub ended_at_ns: u128,
    pub command: Option<String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BashCheckpointCall {
    pub id: i64,
    pub invocation_key: String,
    pub original_cwd: String,
    pub repo_work_dir: Option<String>,
    pub repo_discovery_error: Option<String>,
    pub session_id: String,
    pub tool_use_id: String,
    pub agent_id: AgentId,
    pub start_trace_id: Option<String>,
    pub end_trace_id: Option<String>,
    pub start_time_ns: u128,
    pub end_time_ns: Option<u128>,
    pub command: Option<String>,
    pub metadata: HashMap<String, String>,
}

pub struct BashHistoryDatabase {
    conn: Connection,
    enabled: bool,
}

impl BashHistoryDatabase {
    pub fn global() -> Result<&'static Mutex<BashHistoryDatabase>, GitAiError> {
        let db_result = BASH_HISTORY_DB.get_or_init(|| match Self::new() {
            Ok(db) => Ok(Mutex::new(db)),
            Err(e) => {
                eprintln!("[Error] Failed to initialize bash history database: {}", e);
                match Self::fallback_database() {
                    Ok(db) => Ok(Mutex::new(db)),
                    Err(fallback_error) => {
                        let error_msg = format!(
                            "Failed to initialize bash history database; primary error: {}; fallback error: {}",
                            e, fallback_error
                        );
                        eprintln!("[Error] {}", error_msg);
                        Err(error_msg)
                    }
                }
            }
        });
        match db_result {
            Ok(db_mutex) => Ok(db_mutex),
            Err(error_msg) => Err(GitAiError::Generic(error_msg.clone())),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    fn disabled_in_test_harness() -> bool {
        std::env::var_os("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH").is_none()
            && (std::env::var_os("GIT_AI_TEST_DB_PATH").is_some()
                || std::env::var_os("GITAI_TEST_DB_PATH").is_some())
    }

    #[cfg(any(test, feature = "test-support"))]
    fn disabled_database() -> Self {
        let temp_path = std::env::var_os("GIT_AI_TEST_DB_PATH")
            .or_else(|| std::env::var_os("GITAI_TEST_DB_PATH"))
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("git-ai-bash-history-disabled.db"));
        let db_path = temp_path.with_extension("bash-disabled.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create disabled bash DB directory");
        }
        let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path)
            .expect("Failed to create disabled bash DB");
        BashHistoryDatabase {
            conn,
            enabled: false,
        }
    }

    fn fallback_database() -> Result<Self, GitAiError> {
        let temp_path = std::env::temp_dir().join("git-ai-bash-history-db-failed");
        Self::fallback_database_at(&temp_path)
    }

    fn fallback_database_at(path: &Path) -> Result<Self, GitAiError> {
        Self::open_at_path(path).map_err(|e| {
            GitAiError::Generic(format!(
                "Failed to initialize fallback bash history database at {}: {}",
                path.display(),
                e
            ))
        })
    }

    pub fn open_at_path(path: &Path) -> Result<Self, GitAiError> {
        crate::model::repository::sqlite::open_at_path(path, |conn| {
            let mut db = Self {
                conn,
                enabled: true,
            };
            db.initialize_schema()?;
            Ok(db)
        })
    }

    fn new() -> Result<Self, GitAiError> {
        #[cfg(any(test, feature = "test-support"))]
        if Self::disabled_in_test_harness() {
            return Ok(Self::disabled_database());
        }

        let db_path = Self::database_path()?;
        Self::open_at_path(&db_path)
    }

    fn database_path() -> Result<PathBuf, GitAiError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Ok(test_path) = std::env::var("GIT_AI_TEST_BASH_CHECKPOINT_DB_PATH") {
            return Ok(PathBuf::from(test_path));
        }
        let home = dirs::home_dir().ok_or_else(PersistenceError::home_dir_not_found)?;
        Ok(home
            .join(".git-ai")
            .join("internal")
            .join("bash-checkpoints-db"))
    }

    fn initialize_schema(&mut self) -> Result<(), GitAiError> {
        use crate::model::repository::sqlite;

        sqlite::ensure_schema_metadata_table(&self.conn)?;
        let current_version = sqlite::read_schema_version(&self.conn).unwrap_or(0);
        sqlite::migration_runner(
            &mut self.conn,
            "bash history",
            current_version,
            SCHEMA_VERSION,
            Self::apply_migration,
        )
    }

    fn apply_migration(conn: &mut Connection, from_version: usize) -> Result<(), GitAiError> {
        if from_version >= MIGRATIONS.len() {
            return Err(PersistenceError::no_migration_path(
                "bash history",
                from_version,
                from_version + 1,
            ));
        }

        let tx = conn.transaction()?;
        tx.execute_batch(MIGRATIONS[from_version])?;
        tx.commit()?;
        Ok(())
    }

    pub fn record_start(&mut self, call: &BashCallStart) -> Result<(), GitAiError> {
        if !self.enabled {
            return Ok(());
        }

        self.prune_old_calls_if_due()?;

        let now = unix_now_secs();
        let metadata_json =
            serde_json::to_string(&call.metadata).unwrap_or_else(|_| "{}".to_string());
        let invocation_key = invocation_key(&call.session_id, &call.tool_use_id);
        self.conn.execute(
            r#"
            INSERT INTO bash_checkpoint_calls (
                invocation_key, original_cwd, repo_work_dir, repo_discovery_error,
                session_id, tool_use_id,
                agent_tool, agent_external_id, agent_model,
                start_trace_id, start_time_ns, command, metadata_json,
                created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)
            ON CONFLICT(session_id, tool_use_id, start_trace_id) DO UPDATE SET
                original_cwd = excluded.original_cwd,
                repo_work_dir = excluded.repo_work_dir,
                repo_discovery_error = excluded.repo_discovery_error,
                agent_tool = excluded.agent_tool,
                agent_external_id = excluded.agent_external_id,
                agent_model = excluded.agent_model,
                start_time_ns = excluded.start_time_ns,
                command = COALESCE(excluded.command, bash_checkpoint_calls.command),
                metadata_json = excluded.metadata_json,
                updated_at = excluded.updated_at
            "#,
            params![
                invocation_key,
                call.original_cwd,
                call.repo_work_dir,
                call.repo_discovery_error,
                call.session_id,
                call.tool_use_id,
                call.agent_id.tool,
                call.agent_id.id,
                call.agent_id.model,
                call.start_trace_id,
                ns_to_i64(call.started_at_ns)?,
                call.command,
                metadata_json,
                now as i64,
            ],
        )?;
        Ok(())
    }

    pub fn record_end(&mut self, call: &BashCallEnd) -> Result<(), GitAiError> {
        if !self.enabled {
            return Ok(());
        }

        self.prune_old_calls_if_due()?;

        let now = unix_now_secs();
        let metadata_json =
            serde_json::to_string(&call.metadata).unwrap_or_else(|_| "{}".to_string());
        let end_time_ns = ns_to_i64(call.ended_at_ns)?;

        let updated = if let Some(start_trace_id) = call.start_trace_id.as_ref() {
            self.conn.execute(
                r#"
                UPDATE bash_checkpoint_calls
                SET original_cwd = ?1,
                    repo_work_dir = COALESCE(?2, repo_work_dir),
                    repo_discovery_error = COALESCE(?3, repo_discovery_error),
                    end_trace_id = ?4,
                    end_time_ns = ?5,
                    command = COALESCE(?6, command),
                    metadata_json = ?7,
                    updated_at = ?8
                WHERE session_id = ?9 AND tool_use_id = ?10 AND start_trace_id = ?11
                "#,
                params![
                    call.original_cwd,
                    call.repo_work_dir,
                    call.repo_discovery_error,
                    call.end_trace_id,
                    end_time_ns,
                    call.command,
                    metadata_json,
                    now as i64,
                    call.session_id,
                    call.tool_use_id,
                    start_trace_id,
                ],
            )?
        } else {
            self.conn.execute(
                r#"
                UPDATE bash_checkpoint_calls
                SET original_cwd = ?1,
                    repo_work_dir = COALESCE(?2, repo_work_dir),
                    repo_discovery_error = COALESCE(?3, repo_discovery_error),
                    end_trace_id = ?4,
                    end_time_ns = ?5,
                    command = COALESCE(?6, command),
                    metadata_json = ?7,
                    updated_at = ?8
                WHERE id = (
                    SELECT id
                    FROM bash_checkpoint_calls
                    WHERE session_id = ?9
                      AND tool_use_id = ?10
                      AND end_time_ns IS NULL
                    ORDER BY id DESC
                    LIMIT 1
                )
                "#,
                params![
                    call.original_cwd,
                    call.repo_work_dir,
                    call.repo_discovery_error,
                    call.end_trace_id,
                    end_time_ns,
                    call.command,
                    metadata_json,
                    now as i64,
                    call.session_id,
                    call.tool_use_id,
                ],
            )?
        };

        if updated > 0 {
            return Ok(());
        }

        let start_time_ns = ns_to_i64(call.started_at_ns.unwrap_or(call.ended_at_ns))?;
        let invocation_key = invocation_key(&call.session_id, &call.tool_use_id);
        self.conn.execute(
            r#"
            INSERT INTO bash_checkpoint_calls (
                invocation_key, original_cwd, repo_work_dir, repo_discovery_error,
                session_id, tool_use_id,
                agent_tool, agent_external_id, agent_model,
                start_trace_id, end_trace_id, start_time_ns, end_time_ns,
                command, metadata_json, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)
            ON CONFLICT(session_id, tool_use_id, start_trace_id) DO UPDATE SET
                original_cwd = excluded.original_cwd,
                repo_work_dir = COALESCE(excluded.repo_work_dir, bash_checkpoint_calls.repo_work_dir),
                repo_discovery_error = COALESCE(excluded.repo_discovery_error, bash_checkpoint_calls.repo_discovery_error),
                end_trace_id = excluded.end_trace_id,
                end_time_ns = excluded.end_time_ns,
                command = COALESCE(excluded.command, bash_checkpoint_calls.command),
                metadata_json = excluded.metadata_json,
                updated_at = excluded.updated_at
            "#,
            params![
                invocation_key,
                call.original_cwd,
                call.repo_work_dir,
                call.repo_discovery_error,
                call.session_id,
                call.tool_use_id,
                call.agent_id.tool,
                call.agent_id.id,
                call.agent_id.model,
                call.start_trace_id
                    .clone()
                    .unwrap_or_else(|| call.end_trace_id.clone()),
                call.end_trace_id,
                start_time_ns,
                end_time_ns,
                call.command,
                metadata_json,
                now as i64,
            ],
        )?;
        Ok(())
    }

    pub fn candidates_near_timestamps(
        &self,
        timestamps_ns: &[u128],
        window_ns: u128,
    ) -> Result<Vec<BashCheckpointCall>, GitAiError> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        if timestamps_ns.is_empty() {
            return Ok(Vec::new());
        }

        let min_ts = timestamps_ns
            .iter()
            .copied()
            .min()
            .unwrap_or_default()
            .saturating_sub(window_ns);
        let max_ts = timestamps_ns
            .iter()
            .copied()
            .max()
            .unwrap_or_default()
            .saturating_add(window_ns);

        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, invocation_key, original_cwd, repo_work_dir, repo_discovery_error,
                   session_id, tool_use_id, agent_tool, agent_external_id, agent_model,
                   start_trace_id, end_trace_id, start_time_ns, end_time_ns,
                   command, metadata_json
            FROM bash_checkpoint_calls
            WHERE start_time_ns <= ?1
              AND COALESCE(end_time_ns, start_time_ns) >= ?2
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map(params![ns_to_i64(max_ts)?, ns_to_i64(min_ts)?], row_to_call)?;

        let mut calls = Vec::new();
        for row in rows {
            let call = row?;
            if timestamps_ns
                .iter()
                .any(|ts| distance_to_call_window(*ts, &call) <= window_ns)
            {
                calls.push(call);
            }
        }
        Ok(calls)
    }

    pub fn all_calls_for_test(&self) -> Result<Vec<BashCheckpointCall>, GitAiError> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, invocation_key, original_cwd, repo_work_dir, repo_discovery_error,
                   session_id, tool_use_id, agent_tool, agent_external_id, agent_model,
                   start_trace_id, end_trace_id, start_time_ns, end_time_ns,
                   command, metadata_json
            FROM bash_checkpoint_calls
            ORDER BY id ASC
            "#,
        )?;
        let rows = stmt.query_map([], row_to_call)?;
        let mut calls = Vec::new();
        for row in rows {
            calls.push(row?);
        }
        Ok(calls)
    }

    fn prune_old_calls_if_due(&mut self) -> Result<(), GitAiError> {
        if !self.enabled {
            return Ok(());
        }

        let now = unix_now_secs();
        let last_prune: Option<i64> = self
            .conn
            .query_row(
                "SELECT value FROM schema_metadata WHERE key = 'bash_history_last_prune_ts'",
                [],
                |row| row.get(0),
            )
            .optional()?
            .and_then(|v: String| v.parse().ok());

        if let Some(last) = last_prune
            && now.saturating_sub(last as u64) < PRUNE_INTERVAL_SECS
        {
            return Ok(());
        }

        self.prune_old_calls(now)
    }

    pub fn prune_old_calls(&mut self, now: u64) -> Result<(), GitAiError> {
        if !self.enabled {
            return Ok(());
        }

        let cutoff = now.saturating_sub(RETENTION_SECS);
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM bash_checkpoint_calls WHERE updated_at < ?1",
            params![cutoff as i64],
        )?;
        tx.execute(
            "INSERT OR REPLACE INTO schema_metadata (key, value) VALUES ('bash_history_last_prune_ts', ?1)",
            params![now.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn invocation_key(session_id: &str, tool_use_id: &str) -> String {
    format!("{}:{}", session_id, tool_use_id)
}

pub fn unix_time_ns() -> u128 {
    crate::model::clock::now_nanos()
}

fn unix_now_secs() -> u64 {
    crate::model::clock::now_secs()
}

fn ns_to_i64(ns: u128) -> Result<i64, GitAiError> {
    i64::try_from(ns).map_err(|_| GitAiError::Generic(format!("timestamp too large: {}", ns)))
}

fn i64_to_ns(ns: i64) -> u128 {
    u128::try_from(ns).unwrap_or_default()
}

fn row_to_call(row: &rusqlite::Row<'_>) -> rusqlite::Result<BashCheckpointCall> {
    let metadata_json: String = row.get(15)?;
    let metadata = serde_json::from_str(&metadata_json).unwrap_or_default();
    Ok(BashCheckpointCall {
        id: row.get(0)?,
        invocation_key: row.get(1)?,
        original_cwd: row.get(2)?,
        repo_work_dir: row.get(3)?,
        repo_discovery_error: row.get(4)?,
        session_id: row.get(5)?,
        tool_use_id: row.get(6)?,
        agent_id: AgentId {
            tool: row.get(7)?,
            id: row.get(8)?,
            model: row.get(9)?,
        },
        start_trace_id: row.get(10)?,
        end_trace_id: row.get(11)?,
        start_time_ns: i64_to_ns(row.get(12)?),
        end_time_ns: row.get::<_, Option<i64>>(13)?.map(i64_to_ns),
        command: row.get(14)?,
        metadata,
    })
}

fn distance_to_call_window(timestamp_ns: u128, call: &BashCheckpointCall) -> u128 {
    let start = call.start_time_ns;
    let end = call.end_time_ns.unwrap_or(start);
    if timestamp_ns < start {
        start.saturating_sub(timestamp_ns)
    } else if timestamp_ns > end {
        timestamp_ns.saturating_sub(end)
    } else {
        0
    }
}

#[path = "bash_history_db_tests.rs"]
#[cfg(test)]
mod tests;
