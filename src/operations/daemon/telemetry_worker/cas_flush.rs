//! CAS (content-addressable store) object upload flush.

use super::TelemetryStores;
use super::metrics_flush::default_api_base_and_client;
use crate::clients::api::{CasObject, CasUploadRequest};
use crate::config::DEFAULT_API_BASE_URL;
use crate::model::daemon_control::CasSyncPayload;
use serde_json::Value;

pub(super) fn flush_cas(records: Vec<CasSyncPayload>, stores: TelemetryStores) {
    let (api_base_url, client) = default_api_base_and_client();

    let using_default_api = api_base_url == DEFAULT_API_BASE_URL;
    if using_default_api && !client.is_logged_in() && !client.has_api_key() {
        tracing::debug!("telemetry: skipping CAS flush, not logged in");
        return;
    }

    // Build upload request
    let mut cas_objects = Vec::new();
    for record in &records {
        let content: Value = match serde_json::from_str(&record.data) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(%e, "telemetry: CAS parse error");
                continue;
            }
        };
        // Convert serialized JSON metadata string to HashMap
        let metadata = record
            .metadata
            .as_ref()
            .and_then(|m| serde_json::from_str::<std::collections::HashMap<String, String>>(m).ok())
            .unwrap_or_default();
        cas_objects.push(CasObject {
            content,
            hash: record.hash.clone(),
            metadata,
        });
    }

    if cas_objects.is_empty() {
        return;
    }

    for chunk in cas_objects.chunks(50) {
        let hashes: Vec<String> = chunk.iter().map(|o| o.hash.clone()).collect();
        let request = CasUploadRequest {
            objects: chunk.to_vec(),
        };
        match client.upload_cas(request) {
            Ok(_response) => {
                // Delete successfully uploaded records from the internal DB queue
                // so they don't accumulate as stale entries.
                if let Ok(mut db_lock) = stores.internal.lock() {
                    let _ = db_lock.delete_cas_by_hashes(&hashes);
                }
                tracing::debug!(count = chunk.len(), "telemetry: uploaded CAS objects");
            }
            Err(e) => {
                tracing::warn!(%e, "telemetry: CAS upload error");
            }
        }
    }
}
