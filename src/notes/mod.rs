//! Notes backend module.
//!
//! The notes SQLite store lives at `crate::model::repository::notes_db`
//! used by the HTTP notes backend as both a write queue and a local read cache.
//!
//! `notes::reference_server` is an in-memory reference implementation of the
//! HTTP wire contract — used for local testing, benchmarking, and as
//! documentation of what a real backend must implement.

pub mod reference_server;
