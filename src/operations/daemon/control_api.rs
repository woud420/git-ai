//! Re-exports daemon control-socket DTOs from their canonical model location.
//!
//! All types live in `crate::model::daemon_control`; this shim keeps the
//! `crate::operations::daemon::control_api::*` import paths valid for the
//! 16+ existing consumers.
pub use crate::model::daemon_control::{
    BashSessionQueryResponse, BashSnapshotQueryResponse, CasSyncPayload, ControlRequest,
    ControlResponse, FamilyStatus,
};
