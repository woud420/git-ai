use super::git_backend::SystemGitBackend;
use super::trace_normalizer::TraceNormalizer;
use crate::error::GitAiError;
use crate::model::domain::NormalizedCommand;
use serde_json::Value;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;
use tokio::sync::Mutex;

pub(crate) struct TraceNormalizerWorker {
    normalizer: Arc<Mutex<TraceNormalizer<SystemGitBackend>>>,
}

impl TraceNormalizerWorker {
    pub(crate) fn new(backend: Arc<SystemGitBackend>) -> Self {
        Self {
            normalizer: Arc::new(Mutex::new(TraceNormalizer::new(backend))),
        }
    }

    pub(crate) async fn ingest_payload(
        &self,
        payload: Value,
    ) -> Result<Option<NormalizedCommand>, GitAiError> {
        self.run(move |normalizer| normalizer.ingest_payload(&payload))
            .await
    }

    pub(crate) async fn sweep_orphan(&self, root_sid: String) -> Result<(), GitAiError> {
        self.run(move |normalizer| {
            let _ = normalizer.sweep_orphans_for_roots(&[root_sid]);
            Ok(())
        })
        .await
    }

    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut TraceNormalizer<SystemGitBackend>) -> Result<T, GitAiError>
        + Send
        + 'static,
    ) -> Result<T, GitAiError> {
        let normalizer = Arc::clone(&self.normalizer);
        // The ingest loop awaits each result, retaining trace order while the
        // existing bounded blocking pool absorbs filesystem/configuration I/O.
        let outcome = crate::tokio_runtime::spawn_blocking_result(move || {
            Ok(catch_unwind(AssertUnwindSafe(|| {
                work(&mut normalizer.blocking_lock())
            })))
        })
        .await?;
        match outcome {
            Ok(result) => result,
            // Preserve the ingest worker's existing panic diagnostics and
            // recovery boundary. Tokio's mutex remains usable after unwinding.
            Err(panic) => resume_unwind(panic),
        }
    }
}

#[cfg(test)]
mod tests;
