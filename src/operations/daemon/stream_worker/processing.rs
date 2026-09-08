use super::{Priority, ProcessingTask, StreamWorker};
use crate::model::stream_types::StreamError;
use tokio::time::Duration;

impl StreamWorker {
    /// Process the next task from the queue.
    pub(super) async fn process_next_task(&mut self) {
        // Move any now-ready delayed tasks back to the priority queue
        let now = std::time::Instant::now();
        self.promote_ready_delayed_tasks(now);

        let Some(task) = self.priority_queue.pop() else {
            return;
        };

        // Check if task is ready to be processed (retry delay)
        if let Some(next_retry_at) = task.next_retry_at
            && now < next_retry_at
        {
            self.delayed_tasks.push(task);
            return;
        }

        // Mark as in-flight
        self.in_flight
            .insert((task.canonical_path.clone(), task.stream_kind.clone()));

        // Process the session (spawn blocking to avoid blocking the worker loop)
        let db = self.streams_db.clone();
        let telemetry = self.telemetry_handle.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        let task_clone = task.clone();

        let result = tokio::task::spawn_blocking(move || {
            Self::process_session_blocking(&db, &telemetry, &task_clone, &shutdown_flag)
        })
        .await;

        // Remove from in-flight
        self.in_flight
            .remove(&(task.canonical_path.clone(), task.stream_kind.clone()));

        // Handle result
        match result {
            Ok(Ok(())) => {
                // Success - task is done
            }
            Ok(Err(e)) => {
                // Error - handle retry logic
                self.handle_processing_error(task, e).await;
            }
            Err(e) => {
                // Panic in spawn_blocking
                tracing::error!(target: "git_ai::operations::daemon::stream_worker", error = %e, session_id = %task.session_id, "task panicked");
                self.handle_processing_error(
                    task,
                    StreamError::Fatal {
                        message: format!("task panicked: {}", e),
                    },
                )
                .await;
            }
        }
    }

    /// Handle a processing error with exponential backoff.
    pub(super) async fn handle_processing_error(
        &mut self,
        task: ProcessingTask,
        error: StreamError,
    ) {
        match error {
            StreamError::Transient { message, .. } => {
                // Retry with exponential backoff: 5s, 30s, 5m, 30m
                let retry_count = task.retry_count + 1;
                let max_retries = 4;

                if retry_count >= max_retries {
                    tracing::error!(target: "git_ai::operations::daemon::stream_worker",
                        session_id = %task.session_id,
                        error = %message,
                        "max retries exceeded, dropping task"
                    );
                    if let Err(e) = self.streams_db.record_error(
                        &task.session_id,
                        &task.stream_kind,
                        &task.canonical_path.display().to_string(),
                        &format!("max retries: {}", message),
                    ) {
                        tracing::warn!(target: "git_ai::operations::daemon::stream_worker", session_id = %task.session_id, error = %e, "failed to record error in database");
                    }
                    return;
                }

                let delay = match retry_count {
                    1 => Duration::from_secs(5),
                    2 => Duration::from_secs(30),
                    3 => Duration::from_secs(5 * 60),
                    _ => Duration::from_secs(30 * 60),
                };

                tracing::warn!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %task.session_id,
                    error = %message,
                    retry = retry_count,
                    delay_secs = delay.as_secs(),
                    "transient error, will retry"
                );

                // Re-queue with updated retry count and next_retry_at
                let mut retried_task = task.clone();
                retried_task.retry_count = retry_count;
                retried_task.next_retry_at = Some(std::time::Instant::now() + delay);
                self.delayed_tasks.push(retried_task);
            }
            StreamError::Parse { line, message } => {
                // Parse errors are not retried
                tracing::error!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %task.session_id,
                    line = line,
                    error = %message,
                    "parse error, skipping session"
                );
                if let Err(e) = self.streams_db.record_error(
                    &task.session_id,
                    &task.stream_kind,
                    &task.canonical_path.display().to_string(),
                    &format!("parse line {}: {}", line, message),
                ) {
                    tracing::warn!(target: "git_ai::operations::daemon::stream_worker", session_id = %task.session_id, error = %e, "failed to record error in database");
                }
            }
            StreamError::Fatal { message } => {
                // Fatal errors are not retried
                tracing::error!(target: "git_ai::operations::daemon::stream_worker",
                    session_id = %task.session_id,
                    error = %message,
                    "fatal error, skipping session"
                );
                if let Err(e) = self.streams_db.record_error(
                    &task.session_id,
                    &task.stream_kind,
                    &task.canonical_path.display().to_string(),
                    &format!("fatal: {}", message),
                ) {
                    tracing::warn!(target: "git_ai::operations::daemon::stream_worker", session_id = %task.session_id, error = %e, "failed to record error in database");
                }
            }
        }
    }

    /// Drain immediate priority tasks before shutdown.
    pub(super) async fn drain_immediate_tasks(&mut self) {
        let immediate_tasks = self.take_immediate_tasks();

        tracing::info!(target: "git_ai::operations::daemon::stream_worker", tasks = immediate_tasks.len(), "draining immediate tasks");

        // Process immediate tasks
        for task in immediate_tasks {
            self.in_flight
                .insert((task.canonical_path.clone(), task.stream_kind.clone()));
            let db = self.streams_db.clone();
            let telemetry = self.telemetry_handle.clone();
            let shutdown_flag = self.shutdown_flag.clone();
            let task_clone = task.clone();

            let result = tokio::task::spawn_blocking(move || {
                Self::process_session_blocking(&db, &telemetry, &task_clone, &shutdown_flag)
            })
            .await;

            self.in_flight
                .remove(&(task.canonical_path.clone(), task.stream_kind.clone()));

            match result {
                Err(e) => {
                    tracing::error!(target: "git_ai::operations::daemon::stream_worker", error = %e, session_id = %task.session_id, "drain task panicked");
                }
                Ok(Err(e)) => {
                    tracing::error!(target: "git_ai::operations::daemon::stream_worker", error = %e, session_id = %task.session_id, "drain task processing error");
                }
                Ok(Ok(())) => {}
            }
        }
    }

    /// Removes immediate tasks while preserving lower-priority work.
    pub(super) fn take_immediate_tasks(&mut self) -> Vec<ProcessingTask> {
        let mut immediate_tasks = Vec::new();
        let mut retained_tasks = Vec::new();

        while let Some(task) = self.priority_queue.pop() {
            if task.priority == Priority::Immediate {
                immediate_tasks.push(task);
            } else {
                retained_tasks.push(task);
            }
        }
        self.priority_queue.extend(retained_tasks);

        let mut i = 0;
        while i < self.delayed_tasks.len() {
            if self.delayed_tasks[i].priority == Priority::Immediate {
                immediate_tasks.push(self.delayed_tasks.swap_remove(i));
            } else {
                i += 1;
            }
        }

        immediate_tasks
    }
}
