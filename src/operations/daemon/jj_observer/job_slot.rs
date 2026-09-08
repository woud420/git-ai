#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct JobToken {
    pub(super) revision: u64,
    pub(super) epoch: u64,
}

#[derive(Default)]
pub(super) struct JobSlot {
    pub(super) epoch: u64,
    pub(super) active: Option<JobToken>,
}

impl JobSlot {
    pub(super) fn in_flight(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn reserve(&mut self, revision: u64) -> Result<JobToken, &'static str> {
        if self.in_flight() {
            return Err("Native observation work is already running.");
        }
        self.advance_epoch()?;
        let token = JobToken {
            revision,
            epoch: self.epoch,
        };
        self.active = Some(token);
        Ok(token)
    }

    pub(super) fn cancel(&mut self) -> Result<(), &'static str> {
        self.advance_epoch()
    }

    fn advance_epoch(&mut self) -> Result<(), &'static str> {
        self.epoch = self
            .epoch
            .checked_add(1)
            .ok_or("Observer cancellation epoch exhausted.")?;
        Ok(())
    }

    pub(super) fn finish(&mut self, token: JobToken, revision: u64) -> bool {
        if self.active != Some(token) {
            return false;
        }
        self.active = None;
        token.epoch == self.epoch && token.revision == revision
    }
}

#[cfg(test)]
#[path = "job_slot_tests.rs"]
mod tests;
