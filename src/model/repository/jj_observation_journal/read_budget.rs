use super::{JournalError, invalid};

/// Bounds encoded state and operation BLOBs selected by journal lookups.
/// Bounded metadata columns and SQLite's internal/page I/O are not measured.
#[derive(Debug)]
pub struct ReadBudget {
    initial: usize,
    remaining: usize,
}

impl ReadBudget {
    pub fn new(bytes: usize) -> Self {
        Self {
            initial: bytes,
            remaining: bytes,
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }

    /// Includes selected BLOBs whose subsequent validation failed.
    pub fn consumed(&self) -> usize {
        self.initial - self.remaining
    }

    pub(super) fn charge(&mut self, bytes: usize) -> Result<(), JournalError> {
        self.remaining = self
            .remaining
            .checked_sub(bytes)
            .ok_or_else(|| invalid("stored payload read byte limit exceeded"))?;
        Ok(())
    }
}
