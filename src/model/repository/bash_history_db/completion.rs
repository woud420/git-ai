use super::*;

impl BashHistoryDatabase {
    pub fn record_end(&mut self, call: &BashCallEnd) -> Result<(), GitAiError> {
        if !self.enabled {
            return Ok(());
        }

        self.prune_old_calls_if_due()?;

        let now = unix_now_secs();
        let mut metadata = self.existing_metadata_for_end(call)?;
        metadata.extend(call.metadata.clone());
        let metadata_json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_string());
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

    fn existing_metadata_for_end(
        &self,
        call: &BashCallEnd,
    ) -> Result<HashMap<String, String>, GitAiError> {
        let metadata: Option<String> = self
            .conn
            .query_row(
                r#"
            SELECT metadata_json
            FROM bash_checkpoint_calls
            WHERE session_id = ?1 AND tool_use_id = ?2
              AND ((?3 IS NOT NULL AND start_trace_id = ?3)
                OR (?3 IS NULL AND end_time_ns IS NULL))
            ORDER BY id DESC
            LIMIT 1
            "#,
                params![call.session_id, call.tool_use_id, call.start_trace_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(metadata
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok())
            .unwrap_or_default())
    }
}
