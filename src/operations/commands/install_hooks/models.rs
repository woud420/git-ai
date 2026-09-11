/// Installation status for a tool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStatus {
    /// Tool was not detected on the machine
    NotFound,
    /// Hooks/extensions were successfully installed or updated
    Installed,
    /// Hooks/extensions were already up to date
    AlreadyInstalled,
    /// Installation attempted but failed
    Failed,
}

/// Detailed install result for metrics tracking
#[derive(Debug, Clone)]
pub struct InstallResult {
    pub status: InstallStatus,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
pub(super) struct InstallConfig {
    pub(super) api_base: Option<String>,
    pub(super) api_key: Option<String>,
}
