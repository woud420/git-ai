use std::sync::Arc;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use super::{ControlRequest, ControlResponse};

const MAX_CONTROL_CONNECTIONS: usize = 32;
const MAX_LIVE_CHECKPOINTS: usize = 16;
const MAX_LIVE_CHECKPOINT_BYTES: usize = 128 * 1024 * 1024;

pub(crate) struct ControlAdmission {
    connections: Arc<Semaphore>,
    checkpoints: Arc<Semaphore>,
    checkpoint_bytes: Arc<Semaphore>,
}

pub(crate) struct CheckpointPermit {
    _slot: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

impl Default for ControlAdmission {
    fn default() -> Self {
        Self {
            connections: Arc::new(Semaphore::new(MAX_CONTROL_CONNECTIONS)),
            checkpoints: Arc::new(Semaphore::new(MAX_LIVE_CHECKPOINTS)),
            checkpoint_bytes: Arc::new(Semaphore::new(MAX_LIVE_CHECKPOINT_BYTES)),
        }
    }
}

impl ControlAdmission {
    pub(crate) fn connection(&self) -> Option<OwnedSemaphorePermit> {
        self.connections.clone().try_acquire_owned().ok()
    }

    pub(crate) fn reserve(
        &self,
        request: &ControlRequest,
        encoded_bytes: usize,
    ) -> Result<Option<CheckpointPermit>, ControlResponse> {
        if !matches!(
            request,
            ControlRequest::CheckpointRun { .. } | ControlRequest::CheckpointDeliver { .. }
        ) {
            return Ok(None);
        }
        self.checkpoint(encoded_bytes).map(Some).ok_or_else(|| {
            ControlResponse::err("daemon checkpoint admission is full; retry delivery")
        })
    }

    fn checkpoint(&self, encoded_bytes: usize) -> Option<CheckpointPermit> {
        let bytes = u32::try_from(encoded_bytes).ok()?;
        let slot = self.checkpoints.clone().try_acquire_owned().ok()?;
        let bytes = self
            .checkpoint_bytes
            .clone()
            .try_acquire_many_owned(bytes)
            .ok()?;
        Some(CheckpointPermit {
            _slot: slot,
            _bytes: bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_and_request_limits_release_capacity_after_rejection_and_drop() {
        let admission = ControlAdmission {
            connections: Arc::new(Semaphore::new(16)),
            checkpoints: Arc::new(Semaphore::new(4)),
            checkpoint_bytes: Arc::new(Semaphore::new(128)),
        };
        let large = admission.checkpoint(80).unwrap();
        assert!(admission.checkpoint(49).is_none());
        let small: Vec<_> = (0..3).map(|_| admission.checkpoint(16).unwrap()).collect();
        assert!(admission.checkpoint(0).is_none());
        assert!(
            admission
                .reserve(&ControlRequest::Ping, 1)
                .unwrap()
                .is_none()
        );
        drop(large);
        let replacement = admission.checkpoint(80).unwrap();
        drop(small);
        drop(replacement);
        assert!(admission.checkpoint(128).is_some());
    }
}
