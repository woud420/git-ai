use super::{
    ActorDaemonCoordinator, BashSessionQueryResponse, BashSnapshotQueryResponse, ControlRequest,
    ControlResponse,
};
use crate::error::GitAiError;
use serde_json::json;
use std::path::Path;

impl ActorDaemonCoordinator {
    pub(crate) async fn handle_control_request(&self, request: ControlRequest) -> ControlResponse {
        let result = match request {
            ControlRequest::Ping => Ok(ControlResponse::ok(None, None)),
            request @ (ControlRequest::JjObserverEnable { .. }
            | ControlRequest::JjObserverStatus
            | ControlRequest::JjObserverDisable
            | ControlRequest::JjObserverResume) => {
                Ok(super::jj_observer_control::handle(self, request).await)
            }
            ControlRequest::CheckpointRun { request } => {
                self.ingest_checkpoint_control_payload(*request).await
            }
            ControlRequest::CheckpointDeliver { delivery } => {
                self.ingest_checkpoint_delivery(*delivery).await
            }
            ControlRequest::SyncFamily { repo_working_dir } => {
                self.sync_family(repo_working_dir).await.and_then(|status| {
                    serde_json::to_value(status)
                        .map(|v| ControlResponse::ok(None, Some(v)))
                        .map_err(GitAiError::from)
                })
            }
            ControlRequest::StatusFamily { repo_working_dir } => self
                .status_for_family(repo_working_dir)
                .await
                .and_then(|status| {
                    serde_json::to_value(status)
                        .map(|v| ControlResponse::ok(None, Some(v)))
                        .map_err(GitAiError::from)
                }),
            ControlRequest::StatusDaemon => serde_json::to_value(
                crate::operations::daemon::health::DaemonHealthSnapshot::capture(self),
            )
            .map(|value| ControlResponse::ok(None, Some(value)))
            .map_err(GitAiError::from),
            ControlRequest::SnapshotWatermarks { repo_working_dir } => self
                .watermarks_for_family(repo_working_dir.clone())
                .await
                .and_then(|ws| {
                    let worktree_key = Self::worktree_state_key(Path::new(&repo_working_dir));
                    let worktree_wm = ws.per_worktree.get(&worktree_key).copied();
                    serde_json::to_value(json!({
                        "watermarks": ws.per_file,
                        "worktree_watermark": worktree_wm,
                    }))
                    .map(|v| ControlResponse::ok(None, Some(v)))
                    .map_err(GitAiError::from)
                }),
            ControlRequest::SubmitTelemetry { envelopes } => {
                if let Some(worker) = &self.telemetry_worker {
                    worker.submit_telemetry(envelopes).await;
                }
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::SubmitCas { records } => {
                if let Some(worker) = &self.telemetry_worker {
                    worker.submit_cas(records).await;
                }
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::FlushNotes => {
                // Trigger an immediate notes flush in a blocking task.
                // Route through the worker so the injected notes-db handle is used.
                // Fire-and-forget: the periodic flush loop is the safety net.
                if let Some(worker) = self.telemetry_worker.clone() {
                    tokio::task::spawn_blocking(move || {
                        worker.flush_notes_sync();
                    });
                } else {
                    tokio::task::spawn_blocking(|| {
                        crate::operations::daemon::telemetry_worker::flush_notes_global();
                    });
                }
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::ReingestMetrics { from_ts, to_ts } => {
                let response = if let Some(worker) = &self.telemetry_worker {
                    match worker.reingest_metrics(from_ts, to_ts).await {
                        Ok(reset) => ControlResponse::ok(None, Some(json!({ "reset": reset }))),
                        Err(error) => ControlResponse::err(error),
                    }
                } else {
                    ControlResponse::err("telemetry worker is not available")
                };
                Ok(response)
            }
            ControlRequest::Await { timeout_secs } => {
                let result = self.await_completion(timeout_secs).await;
                serde_json::to_value(result)
                    .map(|v| ControlResponse::ok(None, Some(v)))
                    .map_err(GitAiError::from)
            }
            ControlRequest::BashSessionStart {
                repo_work_dir,
                original_cwd,
                session_id,
                tool_use_id,
                agent_id,
                metadata,
                stat_snapshot,
                trace_id,
                started_at_ns,
                command,
            } => {
                let worktree_key = Self::worktree_state_key(Path::new(&repo_work_dir));
                let original_cwd = original_cwd.unwrap_or_else(|| repo_work_dir.clone());
                if let Ok(db) = self.bash_history_db()
                    && let Ok(mut db_lock) = db.lock()
                    && let Err(e) = db_lock.record_start(
                        &crate::model::repository::bash_history_db::BashCallStart {
                            original_cwd: Self::worktree_state_key(Path::new(&original_cwd)),
                            repo_work_dir: Some(worktree_key.clone()),
                            repo_discovery_error: None,
                            session_id: session_id.clone(),
                            tool_use_id: tool_use_id.clone(),
                            agent_id: agent_id.clone(),
                            start_trace_id: trace_id.clone(),
                            started_at_ns,
                            command: command.clone(),
                            metadata: metadata.clone(),
                        },
                    )
                {
                    tracing::debug!("failed to persist bash session start: {}", e);
                }

                let mut state = self.bash_sessions.lock().unwrap();
                state.start_session(crate::operations::daemon::bash_sessions::BashSessionStart {
                    session_id,
                    tool_use_id,
                    repo_work_dir: worktree_key,
                    agent_id,
                    metadata,
                    stat_snapshot: *stat_snapshot,
                    start_trace_id: trace_id,
                    started_at_ns,
                    command,
                });
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::BashSessionEnd {
                repo_work_dir,
                original_cwd,
                session_id,
                tool_use_id,
                agent_id,
                metadata,
                trace_id,
                ended_at_ns,
                command,
            } => {
                let mut state = self.bash_sessions.lock().unwrap();
                let session = state.end_session(&session_id, &tool_use_id);
                drop(state);

                let worktree_key = session
                    .as_ref()
                    .map(|s| s.repo_work_dir.clone())
                    .unwrap_or_else(|| Self::worktree_state_key(Path::new(&repo_work_dir)));
                let original_cwd = original_cwd
                    .map(|cwd| Self::worktree_state_key(Path::new(&cwd)))
                    .unwrap_or_else(|| worktree_key.clone());
                let start_trace_id = session.as_ref().map(|s| s.start_trace_id.clone());
                let started_at_ns = session.as_ref().map(|s| s.started_at_ns);
                let command = command.or_else(|| session.as_ref().and_then(|s| s.command.clone()));
                let agent_id = session
                    .as_ref()
                    .map(|s| s.agent_id.clone())
                    .unwrap_or(agent_id);
                let metadata = if metadata.is_empty() {
                    session
                        .as_ref()
                        .map(|s| s.metadata.clone())
                        .unwrap_or_default()
                } else {
                    metadata
                };
                if let Ok(db) = self.bash_history_db()
                    && let Ok(mut db_lock) = db.lock()
                    && let Err(e) = db_lock.record_end(
                        &crate::model::repository::bash_history_db::BashCallEnd {
                            original_cwd,
                            repo_work_dir: Some(worktree_key),
                            repo_discovery_error: None,
                            session_id,
                            tool_use_id,
                            agent_id,
                            start_trace_id,
                            end_trace_id: trace_id,
                            started_at_ns,
                            ended_at_ns,
                            command,
                            metadata,
                        },
                    )
                {
                    tracing::debug!("failed to persist bash session end: {}", e);
                }
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::BashHookAttemptStart {
                original_cwd,
                discovered_repo_work_dir,
                repo_discovery_error,
                session_id,
                tool_use_id,
                agent_id,
                metadata,
                trace_id,
                started_at_ns,
                command,
            } => {
                let discovered_repo_work_dir = discovered_repo_work_dir
                    .as_deref()
                    .map(Path::new)
                    .map(Self::worktree_state_key);
                if let Ok(db) = self.bash_history_db()
                    && let Ok(mut db_lock) = db.lock()
                    && let Err(e) = db_lock.record_start(
                        &crate::model::repository::bash_history_db::BashCallStart {
                            original_cwd: Self::worktree_state_key(Path::new(&original_cwd)),
                            repo_work_dir: discovered_repo_work_dir,
                            repo_discovery_error,
                            session_id,
                            tool_use_id,
                            agent_id,
                            start_trace_id: trace_id,
                            started_at_ns,
                            command,
                            metadata,
                        },
                    )
                {
                    tracing::debug!("failed to persist bash hook attempt start: {}", e);
                }
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::BashHookAttemptEnd {
                original_cwd,
                discovered_repo_work_dir,
                repo_discovery_error,
                session_id,
                tool_use_id,
                agent_id,
                metadata,
                trace_id,
                ended_at_ns,
                command,
            } => {
                let discovered_repo_work_dir = discovered_repo_work_dir
                    .as_deref()
                    .map(Path::new)
                    .map(Self::worktree_state_key);
                if let Ok(db) = self.bash_history_db()
                    && let Ok(mut db_lock) = db.lock()
                    && let Err(e) = db_lock.record_end(
                        &crate::model::repository::bash_history_db::BashCallEnd {
                            original_cwd: Self::worktree_state_key(Path::new(&original_cwd)),
                            repo_work_dir: discovered_repo_work_dir,
                            repo_discovery_error,
                            session_id,
                            tool_use_id,
                            agent_id,
                            start_trace_id: None,
                            end_trace_id: trace_id,
                            started_at_ns: None,
                            ended_at_ns,
                            command,
                            metadata,
                        },
                    )
                {
                    tracing::debug!("failed to persist bash hook attempt end: {}", e);
                }
                Ok(ControlResponse::ok(None, None))
            }
            ControlRequest::BashSessionQuery { repo_work_dir } => {
                let state = self.bash_sessions.lock().unwrap();
                let repo_work_dir = Self::worktree_state_key(Path::new(&repo_work_dir));
                let response = match state.query_active_for_repo(&repo_work_dir) {
                    Some((key, session)) => {
                        let data = serde_json::to_value(BashSessionQueryResponse {
                            active: true,
                            agent_id: Some(session.agent_id.clone()),
                            session_id: Some(key.0.clone()),
                            tool_use_id: Some(key.1.clone()),
                            metadata: Some(session.metadata.clone()),
                        })
                        .ok();
                        ControlResponse::ok(None, data)
                    }
                    None => {
                        let data = serde_json::to_value(BashSessionQueryResponse {
                            active: false,
                            agent_id: None,
                            session_id: None,
                            tool_use_id: None,
                            metadata: None,
                        })
                        .ok();
                        ControlResponse::ok(None, data)
                    }
                };
                Ok(response)
            }
            ControlRequest::BashSnapshotQuery {
                session_id,
                tool_use_id,
            } => {
                let state = self.bash_sessions.lock().unwrap();
                let response = match state.get_snapshot(&session_id, &tool_use_id) {
                    Some(snapshot) => {
                        let data = serde_json::to_value(BashSnapshotQueryResponse {
                            found: true,
                            stat_snapshot: Some(snapshot.clone()),
                        })
                        .ok();
                        ControlResponse::ok(None, data)
                    }
                    None => {
                        let data = serde_json::to_value(BashSnapshotQueryResponse {
                            found: false,
                            stat_snapshot: None,
                        })
                        .ok();
                        ControlResponse::ok(None, data)
                    }
                };
                Ok(response)
            }
            ControlRequest::Shutdown => match self.set_checkpoint_acceptance(false) {
                Err(error) => Err(error),
                Ok(owns_gate) => match self.drain_accepted_attribution_work().await {
                    Ok(()) => Ok(ControlResponse::ok(None, None)),
                    Err(error) => {
                        if owns_gate && let Err(reopen_error) = self.set_checkpoint_acceptance(true)
                        {
                            tracing::error!(
                                %reopen_error,
                                "failed reopening checkpoint acceptance after shutdown error"
                            );
                        }
                        Err(error)
                    }
                },
            },
        };

        match result {
            Ok(response) => response,
            Err(error) => ControlResponse::err(error.to_string()),
        }
    }
}
