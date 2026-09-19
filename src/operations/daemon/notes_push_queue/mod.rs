mod completion;
mod scheduler;

use super::actor_types::ActorDaemonCoordinator;
use super::git_op_side_effects::PreparedNotesPush;
use crate::error::GitAiError;
use crate::model::domain::AppliedCommand;
use crate::model::repository::error::PersistenceError;
use crate::operations::git::repository::Repository;
use crate::operations::git::sync_authorship::push_authorship_notes_with_lock;
use completion::Completion;
use scheduler::{Batch, Request, Scheduler};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct NotesPushQueue {
    scheduler: Mutex<Scheduler<Delivery>>,
}

struct Delivery {
    repository: Repository,
    completion: Completion,
}

fn delivery_error(message: impl Into<String>) -> GitAiError {
    PersistenceError::Io {
        operation: "deliver queued notes push",
        path: String::new(),
        kind: std::io::ErrorKind::Other,
        message: message.into(),
    }
    .into()
}

impl ActorDaemonCoordinator {
    pub(crate) fn enqueue_notes_push(
        &self,
        family: &str,
        applied: &AppliedCommand,
        order: u64,
        push: PreparedNotesPush,
    ) -> Result<bool, GitAiError> {
        let Some(coordinator) = self.upgraded_self() else {
            // Bare coordinator test rigs have no task owner; retain their
            // synchronous completion contract.
            let mut first_error = None;
            for destination in &push.destinations {
                if let Err(error) =
                    push_authorship_notes_with_lock(&push.repository, destination, None)
                {
                    first_error.get_or_insert(error);
                }
            }
            return first_error.map_or(Ok(false), Err);
        };
        if push.destinations.len() > scheduler::MAX_DESTINATIONS
            || push
                .destinations
                .iter()
                .any(|target| target.capacity() > 32 * 1024)
        {
            return Err(delivery_error(
                "notes push destination metadata limit exceeded",
            ));
        }
        let context = push.repository.canonical_workdir().to_path_buf();
        let completion = Completion::new(coordinator.clone(), family, applied, order)?;
        let request = Request {
            destinations: push.destinations,
            payload: Delivery {
                repository: push.repository,
                completion,
            },
        };
        let mut scheduler = self
            .notes_push_queue
            .scheduler
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if let Err(mut rejected) = scheduler.admit(family, &context, request) {
            // The caller writes the ordinary command failure record. This
            // request never became queue-owned, so only release its fence.
            rejected.payload.completion.disarm();
            return Err(delivery_error(
                "notes push queue request or destination limit exceeded",
            ));
        }
        drop(scheduler);
        Self::start_ready_notes_pushes(&coordinator);
        Ok(true)
    }

    fn start_ready_notes_pushes(coordinator: &Arc<Self>) {
        loop {
            let batch = coordinator
                .notes_push_queue
                .scheduler
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take_ready();
            let Some(mut batch) = batch else {
                return;
            };
            let coordinator = coordinator.clone();
            std::mem::drop(tokio::task::spawn_blocking(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    coordinator.deliver_notes_push_batch(&mut batch)
                }));
                let result = match outcome {
                    Ok(result) => result,
                    Err(payload) => Err(delivery_error(format!(
                        "notes push worker panic: {}",
                        Self::panic_message(payload)
                    ))),
                };
                // A panic or setup failure must finish every still-owned
                // command before releasing admission and shutdown fences.
                if result.is_err() {
                    for request in &mut batch.requests {
                        request.payload.completion.finish(&result);
                    }
                }
                coordinator
                    .notes_push_queue
                    .scheduler
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .complete(&batch.family, batch.requests.len());
                drop(batch);
                Self::start_ready_notes_pushes(&coordinator);
            }));
        }
    }

    fn deliver_notes_push_batch(&self, batch: &mut Batch<Delivery>) -> Result<(), GitAiError> {
        #[cfg(feature = "test-support")]
        if let Some(path) = std::env::var_os("GIT_AI_TEST_PANIC_IN_NOTES_PUSH_FLAG")
            && std::path::Path::new(&path).exists()
        {
            panic!("test-induced notes push worker panic");
        }
        let lock = self.side_effect_exec_lock(&batch.family)?;
        let repository = &batch.requests[0].payload.repository;
        let results: Vec<_> = batch
            .destinations
            .iter()
            .map(|destination| {
                push_authorship_notes_with_lock(repository, destination, Some(&lock))
            })
            .collect();
        for request in &mut batch.requests {
            let result = request
                .destinations
                .iter()
                .filter_map(|destination| {
                    batch
                        .destinations
                        .iter()
                        .position(|candidate| candidate == destination)
                })
                .map(|index| &results[index])
                .find(|result| result.is_err())
                .unwrap_or(&Ok(()));
            request.payload.completion.finish(result);
        }
        Ok(())
    }
}
