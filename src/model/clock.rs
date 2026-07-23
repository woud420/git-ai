//! Shared wall-clock helpers.
//!
//! A single, std-only source of truth for "now as an integer" reads, used
//! everywhere from checkpoint timestamps to cache freshness checks. All three
//! functions treat a pre-epoch system clock the same way: fall back to `0`
//! (via `unwrap_or_default()`) rather than panicking, since a clock behind
//! `UNIX_EPOCH` is not something callers can meaningfully recover from.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current Unix time in whole seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Current Unix time in whole milliseconds.
pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Current Unix time in whole nanoseconds.
pub fn now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
