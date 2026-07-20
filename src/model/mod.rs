//! Core entities, DTOs, and persistence-facing types.
//!
//! `model` holds the data shapes that flow between subsystems: the authorship
//! log and its serialization, working-log checkpoints, the daemon's normalized
//! command/event domain, stream/transcript types, and API request/response
//! DTOs. SQLite access lives in [`repository`].
//!
//! The previous module locations re-export from here (e.g.
//! `crate::authorship::working_log`, `crate::api::types`), so both paths are
//! valid during the migration.

pub mod api_types;
pub mod authorship_log;
pub mod authorship_log_serialization;
pub mod domain;
pub mod repository;
pub mod stream_types;
pub mod working_log;
