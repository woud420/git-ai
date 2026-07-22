//! Metrics data types: event structs, position-encoded serialization, and
//! common attributes. These are pure model types (serde/std only) shared
//! between the metrics emission layer (`crate::metrics`) and the persistence
//! layer (`crate::model::repository::metrics_db`).

pub mod attrs;
pub mod events;
pub mod pos_encoded;
pub mod types;
