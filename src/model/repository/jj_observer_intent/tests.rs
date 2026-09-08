use super::{ObserverStoreError, StoredIntent, StoredTarget, load, replace};
use crate::model::jj_observation::JJ_OBSERVATION_READER_PROFILE;
use crate::model::jj_observer::{JjObserverError, JjObserverTarget};
use crate::model::repository::sqlite;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

mod support;
use support::*;
mod basics;
mod concurrency;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod permissions;
mod schema;
mod validation;

mod empty_diagnostic;
