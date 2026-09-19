use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

pub(super) const MAX_REQUESTS: usize = 128;
pub(super) const MAX_DESTINATIONS: usize = 8;
pub(super) const MAX_ACTIVE_FAMILIES: usize = 4;

pub(super) struct Request<T> {
    pub destinations: Vec<String>,
    pub payload: T,
}

pub(super) struct Batch<T> {
    pub family: String,
    context: PathBuf,
    pub destinations: Vec<String>,
    pub requests: Vec<Request<T>>,
}

pub(super) struct Scheduler<T> {
    pending: VecDeque<Batch<T>>,
    active: HashSet<String>,
    outstanding: usize,
}

impl<T> Default for Scheduler<T> {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            active: HashSet::new(),
            outstanding: 0,
        }
    }
}

impl<T> Scheduler<T> {
    pub fn admit(
        &mut self,
        family: &str,
        context: &Path,
        request: Request<T>,
    ) -> Result<(), Request<T>> {
        if self.outstanding == MAX_REQUESTS {
            return Err(request);
        }
        let family_destinations: HashSet<_> = self
            .pending
            .iter()
            .filter(|batch| batch.family == family)
            .flat_map(|batch| &batch.destinations)
            .chain(&request.destinations)
            .collect();
        if family_destinations.len() > MAX_DESTINATIONS {
            return Err(request);
        }
        // Worktree-specific Git config can select different credentials or
        // transports. Coalesce only requests with the same execution context.
        let pending = self
            .pending
            .iter_mut()
            .find(|batch| batch.family == family && batch.context == context);
        let mut destinations = pending
            .as_ref()
            .map(|batch| batch.destinations.clone())
            .unwrap_or_default();
        for destination in &request.destinations {
            if !destinations.contains(destination) {
                destinations.push(destination.clone());
            }
        }
        if destinations.is_empty() || destinations.len() > MAX_DESTINATIONS {
            return Err(request);
        }
        if let Some(batch) = pending {
            batch.destinations = destinations;
            batch.requests.push(request);
        } else {
            self.pending.push_back(Batch {
                family: family.to_string(),
                context: context.to_path_buf(),
                destinations,
                requests: vec![request],
            });
        }
        self.outstanding += 1;
        Ok(())
    }

    pub fn take_ready(&mut self) -> Option<Batch<T>> {
        if self.active.len() == MAX_ACTIVE_FAMILIES {
            return None;
        }
        let index = self
            .pending
            .iter()
            .position(|batch| !self.active.contains(&batch.family))?;
        let batch = self.pending.remove(index)?;
        self.active.insert(batch.family.clone());
        Some(batch)
    }

    pub fn complete(&mut self, family: &str, requests: usize) {
        self.active.remove(family);
        self.outstanding -= requests;
    }
}

#[cfg(test)]
mod tests;
