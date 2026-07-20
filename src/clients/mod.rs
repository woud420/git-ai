//! Clients for external services and processes: the git-ai HTTP API
//! (metrics/logs/notes/CAS upload), authentication and credential storage,
//! and the shared HTTP helper.

pub mod api;
pub mod auth;
pub mod http;
