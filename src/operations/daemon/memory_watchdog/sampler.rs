use super::rss::{current_rss_bytes, peak_rss_bytes};
use std::io;

#[derive(Debug)]
pub(super) struct MemoryUsage {
    pub current_bytes: u64,
    pub peak_bytes: Option<u64>,
}

pub(super) struct RssSampler {
    #[cfg(feature = "test-support")]
    current: Option<TestSamples>,
    #[cfg(feature = "test-support")]
    peak: Option<TestSamples>,
}

impl RssSampler {
    pub(super) fn new() -> Self {
        Self {
            #[cfg(feature = "test-support")]
            current: TestSamples::from_env("GIT_AI_TEST_DAEMON_CURRENT_RSS_MB_SEQUENCE"),
            #[cfg(feature = "test-support")]
            peak: TestSamples::from_env("GIT_AI_TEST_DAEMON_PEAK_RSS_MB_SEQUENCE"),
        }
    }

    pub(super) fn sample(&mut self) -> io::Result<MemoryUsage> {
        #[cfg(feature = "test-support")]
        let (current, peak) = (
            sample_or(&mut self.current, current_rss_bytes),
            sample_or(&mut self.peak, peak_rss_bytes),
        );
        #[cfg(not(feature = "test-support"))]
        let (current, peak) = (current_rss_bytes(), peak_rss_bytes());
        Ok(MemoryUsage {
            current_bytes: current?,
            // Historical diagnostics must not disable current-memory enforcement.
            peak_bytes: peak.ok(),
        })
    }
}

#[cfg(feature = "test-support")]
struct TestSamples {
    remaining: std::collections::VecDeque<u64>,
    last: u64,
}

#[cfg(feature = "test-support")]
impl TestSamples {
    fn from_env(key: &str) -> Option<Self> {
        let remaining = std::env::var(key)
            .ok()?
            .split(',')
            .map(|part| part.trim().parse::<u64>().ok())
            .collect::<Option<std::collections::VecDeque<_>>>()?;
        Some(Self {
            last: *remaining.front()?,
            remaining,
        })
    }
}

#[cfg(feature = "test-support")]
fn sample_or(
    samples: &mut Option<TestSamples>,
    native: fn() -> io::Result<u64>,
) -> io::Result<u64> {
    let Some(samples) = samples else {
        return native();
    };
    samples.last = samples.remaining.pop_front().unwrap_or(samples.last);
    samples
        .last
        .checked_mul(crate::config::MEBIBYTE_BYTES)
        .ok_or_else(|| io::Error::other("test RSS sample overflowed bytes"))
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use super::*;

    fn samples(value: u64) -> Option<TestSamples> {
        Some(TestSamples {
            remaining: [value].into(),
            last: value,
        })
    }

    #[test]
    fn unavailable_peak_does_not_discard_current_memory_usage() {
        let mut sampler = RssSampler {
            current: samples(100),
            peak: samples(u64::MAX),
        };
        let usage = sampler.sample().unwrap();
        assert_eq!(usage.current_bytes, 100 * crate::config::MEBIBYTE_BYTES);
        assert_eq!(usage.peak_bytes, None);
    }

    #[test]
    fn unavailable_current_memory_never_falls_back_to_the_peak() {
        let mut sampler = RssSampler {
            current: samples(u64::MAX),
            peak: samples(100),
        };
        assert!(sampler.sample().is_err());
    }
}
