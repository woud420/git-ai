use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::fs;
use std::time::Instant;

// Deterministic xorshift64 PRNG keeps benchmark workloads reproducible without rand.
struct DeterministicRng(u64);

impl DeterministicRng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn gen_range(&mut self, max: usize) -> usize {
        (self.next() as usize) % max.max(1)
    }
}

fn extract_timing(data: &str, key: &str) -> Option<u64> {
    for line in data.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(key)
            && let Some(val) = trimmed.split('=').nth(1)
        {
            return val.trim_end_matches("ms").parse().ok();
        }
    }
    None
}

mod determinism_and_small_rebases;
mod diff_reconstruction;
mod heavy_rebases;
mod mixed_workload;
mod monorepo_rebase;
mod plumbing_rebase;
mod realistic_monorepo;
