//! Clients for external services and processes: the git-ai HTTP API
//! (metrics/logs/notes/CAS upload), authentication and credential storage,
//! the shared HTTP helper, and the git spawn layer.

pub mod api;
pub mod auth;
pub mod git_cli;
pub mod http;
