use super::{ActorDaemonCoordinator, AppliedCommand, GitAiError, delivery_error};
use crate::operations::daemon::daemon_config::TestCompletionLogEntry;
use std::sync::Arc;

const MAX_COMPLETION_BYTES: usize = 64 * 1024;

pub(super) struct Completion {
    coordinator: Arc<ActorDaemonCoordinator>,
    entry: TestCompletionLogEntry,
    order: u64,
    finished: bool,
}

impl Completion {
    pub fn new(
        coordinator: Arc<ActorDaemonCoordinator>,
        family: &str,
        applied: &AppliedCommand,
        order: u64,
    ) -> Result<Self, GitAiError> {
        let entry = ActorDaemonCoordinator::command_completion_entry(family, applied, &Ok(()));
        if retained_bytes(&entry) > MAX_COMPLETION_BYTES {
            return Err(delivery_error(
                "notes push completion metadata limit exceeded",
            ));
        }
        // The family drain still owns its fence here: sync never observes a
        // gap between sequenced work and detached delivery.
        coordinator.begin_family_effect(family)?;
        Ok(Self {
            coordinator,
            entry,
            order,
            finished: false,
        })
    }

    pub fn disarm(&mut self) {
        self.finished = true;
    }

    pub fn finish(&mut self, result: &Result<(), GitAiError>) {
        if self.finished {
            return;
        }
        self.coordinator.record_and_log_side_effect_result(
            &self.entry.family_key,
            self.order,
            "notes_push_delivery",
            "queued notes push failed",
            result,
        );
        self.entry.status = if result.is_ok() { "ok" } else { "error" }.to_string();
        self.entry.error = result.as_ref().err().map(ToString::to_string);
        if let Err(error) = self
            .coordinator
            .maybe_append_test_completion_log(&self.entry.family_key, &self.entry)
        {
            let _ = self.coordinator.record_side_effect_error(
                &self.entry.family_key,
                self.order,
                &error,
            );
            tracing::error!(%error, family = %self.entry.family_key, "queued push completion log write failed");
        }
        self.finished = true;
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        if !self.finished {
            self.finish(&Err(delivery_error(
                "notes push delivery ended without a result",
            )));
        }
        if let Err(error) = self.coordinator.end_family_effect(&self.entry.family_key) {
            tracing::error!(%error, family = %self.entry.family_key, "failed releasing notes push family fence");
        }
    }
}

fn retained_bytes(entry: &TestCompletionLogEntry) -> usize {
    let strings = [
        Some(&entry.family_key),
        Some(&entry.kind),
        entry.primary_command.as_ref(),
        entry.test_sync_session.as_ref(),
        Some(&entry.status),
        entry.error.as_ref(),
        entry.commit_skip_reason.as_ref(),
    ];
    strings
        .into_iter()
        .flatten()
        .map(String::capacity)
        .sum::<usize>()
        + entry
            .semantic_events
            .iter()
            .chain(&entry.commit_shas)
            .map(String::capacity)
            .sum::<usize>()
        + (entry.semantic_events.capacity() + entry.commit_shas.capacity()) * size_of::<String>()
        + size_of::<TestCompletionLogEntry>()
}
