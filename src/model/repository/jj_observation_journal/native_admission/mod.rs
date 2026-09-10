//! Canonical structural admission packets; native ancestry and live-source authority are checked above storage.

pub(super) mod bounded;
mod types;
pub(crate) use types::{NativeAdmissionCursor, StoredNativeAdmission};

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod codec;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod payload;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod read;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod request;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod snapshot;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod transaction;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod validate;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use request::{ExpectedAdmissionCursor, NativeAdmissionScope, PreparedNativeAdmission};
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use snapshot::StoredAdmissionSnapshot;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(crate) use transaction::NativeAdmissionOutcome;

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
use super::{JjObservationJournal, JournalError, ReadBudget};
#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests;
