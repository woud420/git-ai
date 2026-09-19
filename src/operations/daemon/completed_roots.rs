use std::collections::{HashMap, VecDeque};

const RETENTION_LIMIT: usize = 4096;

#[derive(Debug, Default, Clone)]
pub(crate) struct CompletedRoots {
    generations: HashMap<String, u64>,
    order: VecDeque<(String, u64)>,
    next_generation: u64,
}

impl CompletedRoots {
    pub(crate) fn contains(&self, root: &str) -> bool {
        self.generations.contains_key(root)
    }

    pub(crate) fn forget(&mut self, root: &str) {
        self.generations.remove(root);
    }

    pub(crate) fn remember(&mut self, root: &str) {
        if self.contains(root) {
            return;
        }
        let generation = self.next_generation;
        self.next_generation = generation.wrapping_add(1);
        self.generations.insert(root.to_string(), generation);
        self.order.push_back((root.to_string(), generation));
        if self.order.len() > RETENTION_LIMIT {
            let (oldest, generation) = self.order.pop_front().unwrap();
            // Reactivation leaves a stale FIFO entry. Its eviction must not
            // forget a newer completion, and forgetting must never scan ingress.
            if self.generations.get(&oldest) == Some(&generation) {
                self.generations.remove(&oldest);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_roots_eviction_preserves_recompleted_root_and_bounds_both_collections() {
        let mut roots = CompletedRoots::default();
        roots.remember("reactivated");
        roots.forget("reactivated");
        roots.remember("reactivated");
        for index in 0..RETENTION_LIMIT - 1 {
            roots.remember(&index.to_string());
        }
        assert!(roots.contains("reactivated"));
        assert_eq!(roots.order.len(), RETENTION_LIMIT);
        assert_eq!(roots.generations.len(), RETENTION_LIMIT);
        roots.remember("newest");
        assert!(!roots.contains("reactivated"));
        assert!(roots.contains("newest"));
        assert_eq!(roots.order.len(), RETENTION_LIMIT);
        assert_eq!(roots.generations.len(), RETENTION_LIMIT);
    }
}
