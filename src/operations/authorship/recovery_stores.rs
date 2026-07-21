/// Handles to the two database stores consumed by the attribution-recovery chain.
///
/// Constructed once at process edges (daemon init, CLI entry) via [`RecoveryStores::resolve`]
/// and threaded into [`super::attribution_recovery`] functions so no `::global()` call
/// occurs inside the library. Copy: pass by value freely.
#[derive(Clone, Copy)]
pub(crate) struct RecoveryStores {
    /// Metrics DB handle, or `None` when the store is unavailable (unit tests).
    pub(crate) metrics:
        Option<&'static std::sync::Mutex<crate::model::repository::metrics_db::MetricsDatabase>>,
    /// Bash-history DB handle, or `None` when the store is unavailable or errored at init.
    pub(crate) bash_history: Option<
        &'static std::sync::Mutex<crate::model::repository::bash_history_db::BashHistoryDatabase>,
    >,
}

impl RecoveryStores {
    /// Resolve store handles by calling the global singletons.
    ///
    /// Cheap after the first call (cached `OnceLock` gets); resolve at process
    /// edges and pass the copied value into attribution-recovery functions.
    pub(crate) fn resolve() -> Self {
        use crate::model::repository::{
            bash_history_db::BashHistoryDatabase, metrics_db::MetricsDatabase,
        };
        Self {
            metrics: MetricsDatabase::global().ok(),
            bash_history: BashHistoryDatabase::global().ok(),
        }
    }
}
