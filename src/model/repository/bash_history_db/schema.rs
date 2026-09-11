pub(super) const MIGRATIONS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS bash_checkpoint_calls (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        invocation_key TEXT NOT NULL,
        repo_work_dir TEXT NOT NULL,
        session_id TEXT NOT NULL,
        tool_use_id TEXT NOT NULL,
        agent_tool TEXT NOT NULL,
        agent_external_id TEXT NOT NULL,
        agent_model TEXT NOT NULL,
        start_trace_id TEXT,
        end_trace_id TEXT,
        start_time_ns INTEGER NOT NULL,
        end_time_ns INTEGER,
        command TEXT,
        metadata_json TEXT NOT NULL DEFAULT '{}',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_bash_calls_repo_time
        ON bash_checkpoint_calls(repo_work_dir, start_time_ns, end_time_ns);

    CREATE UNIQUE INDEX IF NOT EXISTS idx_bash_calls_invocation
        ON bash_checkpoint_calls(session_id, tool_use_id, start_trace_id);

    CREATE INDEX IF NOT EXISTS idx_bash_calls_time
        ON bash_checkpoint_calls(start_time_ns, end_time_ns);
"#,
    r#"
    ALTER TABLE bash_checkpoint_calls RENAME TO bash_checkpoint_calls_v1;

    CREATE TABLE IF NOT EXISTS bash_checkpoint_calls (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        invocation_key TEXT NOT NULL,
        original_cwd TEXT NOT NULL,
        repo_work_dir TEXT,
        repo_discovery_error TEXT,
        session_id TEXT NOT NULL,
        tool_use_id TEXT NOT NULL,
        agent_tool TEXT NOT NULL,
        agent_external_id TEXT NOT NULL,
        agent_model TEXT NOT NULL,
        start_trace_id TEXT,
        end_trace_id TEXT,
        start_time_ns INTEGER NOT NULL,
        end_time_ns INTEGER,
        command TEXT,
        metadata_json TEXT NOT NULL DEFAULT '{}',
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    INSERT INTO bash_checkpoint_calls (
        id, invocation_key, original_cwd, repo_work_dir, repo_discovery_error,
        session_id, tool_use_id, agent_tool, agent_external_id, agent_model,
        start_trace_id, end_trace_id, start_time_ns, end_time_ns,
        command, metadata_json, created_at, updated_at
    )
    SELECT
        id, invocation_key, repo_work_dir, repo_work_dir, NULL,
        session_id, tool_use_id, agent_tool, agent_external_id, agent_model,
        start_trace_id, end_trace_id, start_time_ns, end_time_ns,
        command, metadata_json, created_at, updated_at
    FROM bash_checkpoint_calls_v1;

    DROP TABLE bash_checkpoint_calls_v1;

    CREATE INDEX IF NOT EXISTS idx_bash_calls_repo_time
        ON bash_checkpoint_calls(repo_work_dir, start_time_ns, end_time_ns);

    CREATE UNIQUE INDEX IF NOT EXISTS idx_bash_calls_invocation
        ON bash_checkpoint_calls(session_id, tool_use_id, start_trace_id);

    CREATE INDEX IF NOT EXISTS idx_bash_calls_time
        ON bash_checkpoint_calls(start_time_ns, end_time_ns);
"#,
];
