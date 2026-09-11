use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub(super) enum UpgradeAction {
    UpgradeAvailable,
    AlreadyLatest,
    RunningNewerVersion,
    ForceReinstall,
}

#[derive(Debug, Clone)]
pub(super) struct ChannelRelease {
    pub(super) tag: String,
    pub(super) semver: String,
    pub(super) checksum: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct UpdateCache {
    pub(super) last_checked_at: u64,
    pub(super) available_tag: Option<String>,
    pub(super) available_semver: Option<String>,
    pub(super) channel: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ChannelInfo {
    pub(super) version: String,
    pub(super) checksum: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct ReleasesResponse {
    pub(super) channels: HashMap<String, ChannelInfo>,
}

/// Result of checking whether a daemon-initiated update is available.
#[derive(Debug, PartialEq)]
pub enum DaemonUpdateCheckResult {
    /// No update is needed (already latest, checks disabled, or not yet time to check).
    NoUpdate,
    /// An update is available and auto-updates are enabled.
    UpdateReady,
}

#[cfg(windows)]
pub(super) type WindowsHandle = *mut std::ffi::c_void;

#[cfg(windows)]
#[repr(C)]
pub(super) struct ProcessEntry32W {
    pub(super) dw_size: u32,
    pub(super) cnt_usage: u32,
    pub(super) th32_process_id: u32,
    pub(super) th32_default_heap_id: usize,
    pub(super) th32_module_id: u32,
    pub(super) cnt_threads: u32,
    pub(super) th32_parent_process_id: u32,
    pub(super) pc_pri_class_base: i32,
    pub(super) dw_flags: u32,
    pub(super) sz_exe_file: [u16; WINDOWS_MAX_PATH],
}

#[cfg(windows)]
pub(super) const WINDOWS_MAX_PATH: usize = 260;
