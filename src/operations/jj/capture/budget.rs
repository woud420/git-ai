use super::JjCaptureError as E;
use crate::operations::jj::baseline::MAX_JJ_BASELINE_RAW_BYTES;
use crate::regular_file::MetadataReadBudget;
use std::ffi::CStr;
use std::fs::File;
use std::io;
use std::os::fd::RawFd;
use std::time::Instant;

pub(super) struct CaptureLimits {
    pub(super) directory_components: usize,
    pub(super) retained_edges: usize,
    pub(super) directory_open_attempts: usize,
    pub(super) live_directory_descriptors: usize,
    pub(super) head_calls_per_pass: usize,
    pub(super) head_calls_total: usize,
    pub(super) retained_anchor_bytes: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            directory_components: 256,
            retained_edges: 256,
            directory_open_attempts: 256,
            live_directory_descriptors: 256,
            head_calls_per_pass: 36,
            head_calls_total: 72,
            retained_anchor_bytes: MAX_JJ_BASELINE_RAW_BYTES,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct CaptureCounters {
    pub(super) directory_components: usize,
    pub(super) retained_edges: usize,
    pub(super) directory_open_attempts: usize,
    pub(super) live_directory_descriptors: usize,
    pub(super) peak_live_directory_descriptors: usize,
    pub(super) head_calls: [usize; 2],
    pub(super) retained_anchor_bytes: usize,
}

pub(super) struct CaptureBudget {
    limits: CaptureLimits,
    counters: CaptureCounters,
    deadline: Instant,
    pub(super) metadata: MetadataReadBudget,
}

impl CaptureBudget {
    pub(super) fn new(deadline: Instant) -> Self {
        Self::with_limits(deadline, CaptureLimits::default())
    }

    pub(super) fn with_limits(deadline: Instant, limits: CaptureLimits) -> Self {
        Self {
            limits,
            counters: CaptureCounters::default(),
            deadline,
            metadata: MetadataReadBudget::new(10 * 1024 * 1024, 192, deadline),
        }
    }

    #[cfg(test)]
    pub(super) fn counters(&self) -> CaptureCounters {
        self.counters
    }

    pub(super) fn check(&self, hooks: &mut impl CaptureHooks) -> Result<(), E> {
        if hooks.now() >= self.deadline {
            return Err(E::invalid(
                "deadline",
                "absolute cooperative deadline expired",
            ));
        }
        Ok(())
    }

    pub(super) fn component(&mut self, hooks: &mut impl CaptureHooks) -> Result<(), E> {
        self.check(hooks)?;
        if self.counters.directory_components >= self.limits.directory_components {
            return Err(E::invalid("directory", "component count limit exceeded"));
        }
        self.counters.directory_components += 1;
        Ok(())
    }

    pub(super) fn check_edge_capacity(&self) -> Result<(), E> {
        if self.counters.retained_edges >= self.limits.retained_edges {
            return Err(E::invalid("directory", "retained edge limit exceeded"));
        }
        Ok(())
    }

    pub(super) fn retain_edge(&mut self) {
        self.counters.retained_edges += 1;
    }

    pub(super) fn begin_directory_open(&mut self, hooks: &mut impl CaptureHooks) -> Result<(), E> {
        self.check(hooks)?;
        if self.counters.directory_open_attempts >= self.limits.directory_open_attempts {
            return Err(E::invalid("directory", "open attempt limit exceeded"));
        }
        if self.counters.live_directory_descriptors >= self.limits.live_directory_descriptors {
            return Err(E::invalid("directory", "live descriptor limit exceeded"));
        }
        // Reserve the live slot before the syscall; neither refusal invents an attempt.
        self.counters.directory_open_attempts += 1;
        self.counters.live_directory_descriptors += 1;
        self.counters.peak_live_directory_descriptors = self
            .counters
            .peak_live_directory_descriptors
            .max(self.counters.live_directory_descriptors);
        Ok(())
    }

    pub(super) fn close_directories(&mut self, count: usize) {
        self.counters.live_directory_descriptors -= count;
    }

    pub(super) fn head_call(
        &mut self,
        pass: usize,
        hooks: &mut impl CaptureHooks,
    ) -> Result<(), E> {
        self.check(hooks)?;
        if self.counters.head_calls[pass] >= self.limits.head_calls_per_pass
            || self.counters.head_calls.iter().sum::<usize>() >= self.limits.head_calls_total
        {
            return Err(E::invalid("heads", "raw directory call limit exceeded"));
        }
        self.counters.head_calls[pass] += 1;
        Ok(())
    }

    pub(super) fn anchor_remaining(&self) -> usize {
        self.limits
            .retained_anchor_bytes
            .saturating_sub(self.counters.retained_anchor_bytes)
    }

    pub(super) fn retain_anchor_bytes(&mut self, bytes: usize) -> Result<(), E> {
        if bytes > self.anchor_remaining() {
            return Err(E::invalid(
                "evidence",
                "retained anchor byte limit exceeded",
            ));
        }
        self.counters.retained_anchor_bytes += bytes;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CapturePhase {
    InitialSamplesRead,
    EvidenceVerified,
    FinalSamplesRead,
}

pub(super) trait CaptureHooks {
    fn phase(&mut self, _phase: CapturePhase) {}
    fn now(&mut self) -> Instant {
        Instant::now()
    }

    fn open_directory(&mut self, parent: RawFd, name: &CStr) -> io::Result<File> {
        crate::unix_directory::open_directory_at(parent, name)
    }
}

pub(super) struct DirectCapture;
impl CaptureHooks for DirectCapture {}
