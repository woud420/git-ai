use super::{
    CaptureOutcome, JjObservationJournal, JournalError, MAX_METADATA_BYTES, MAX_RECORD_BYTES,
    StoredOperation, codec, graph, invalid, load_state, records, sql_error,
};
use crate::model::jj_observation::{
    JjObservationBatch, JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, is_root,
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema_version: u16,
    reader_profile: String,
    source_id: String,
    expected_generation: u64,
    expected_observed_heads: Vec<String>,
    captured_integrated_heads: Vec<String>,
    operation_ids: Vec<String>,
    digest: String,
}

impl Receipt {
    fn for_batch(batch: &JjObservationBatch, digest: String) -> Self {
        Self {
            schema_version: batch.schema_version,
            reader_profile: batch.reader_profile.clone(),
            source_id: batch.source_id.clone(),
            expected_generation: batch.expected_generation,
            expected_observed_heads: batch.expected_observed_heads.clone(),
            captured_integrated_heads: batch.captured_integrated_heads.clone(),
            operation_ids: batch
                .operations
                .iter()
                .map(|operation| operation.operation_id.clone())
                .collect(),
            digest,
        }
    }
}

impl JjObservationJournal {
    pub fn capture(&mut self, batch: &JjObservationBatch) -> Result<CaptureOutcome, JournalError> {
        batch.validate()?;
        let bytes = codec::encode(batch, MAX_JJ_OBSERVATION_BATCH_BYTES)?;
        let expected_receipt = Receipt::for_batch(batch, codec::checksum(&bytes));
        drop(bytes);
        let prepared = prepare_operations(batch)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| sql_error("begin capture", error))?;
        let mut state = load_state(&tx, &batch.source_id)?;
        let known = load_relevant_records(&tx, batch)?;
        check_collisions(&tx, batch, &known)?;
        if let Some(receipt) = load_receipt(&tx, &batch.source_id, &expected_receipt.digest)? {
            if receipt != expected_receipt {
                return Err(invalid("stored batch receipt identity mismatch"));
            }
            if batch
                .operations
                .iter()
                .any(|operation| !known.contains_key(&operation.operation_id))
                || batch
                    .captured_integrated_heads
                    .iter()
                    .chain(&batch.expected_observed_heads)
                    .chain(
                        batch
                            .operations
                            .iter()
                            .flat_map(|operation| &operation.parent_ids),
                    )
                    .any(|head| !is_root(head) && !known.contains_key(head))
            {
                return Err(invalid("captured receipt has an operation gap"));
            }
            return Ok(CaptureOutcome::AlreadyCaptured);
        }
        if state.generation != batch.expected_generation {
            return Err(invalid(
                "expected generation does not match current generation",
            ));
        }
        let mut expected_heads = batch.expected_observed_heads.clone();
        expected_heads.sort();
        if state.observed_heads != expected_heads {
            return Err(invalid(
                "expected observed frontier does not match current frontier",
            ));
        }
        let ordered = graph::order_new(batch, &known)?;
        state.generation = state
            .generation
            .checked_add(1)
            .ok_or_else(|| invalid("generation limit exceeded"))?;
        let first_sequence = state.pending_operations;
        state.pending_operations = state
            .pending_operations
            .checked_add(ordered.len() as u64)
            .filter(|count| *count <= i64::MAX as u64)
            .ok_or_else(|| invalid("stored operation count limit exceeded"))?;
        state.observed_heads = batch.captured_integrated_heads.clone();
        state.observed_heads.sort();
        let encoded_state = codec::encode(&state, MAX_METADATA_BYTES)?;
        tx.execute(
            "INSERT INTO jj_sources (source_id, state, checksum) VALUES (?1, ?2, ?3)
             ON CONFLICT(source_id) DO UPDATE SET state = excluded.state, checksum = excluded.checksum",
            params![batch.source_id, encoded_state, codec::checksum(&encoded_state)],
        ).map_err(|error| sql_error("persist progress", error))?;
        for (index, operation) in ordered.iter().enumerate() {
            let payload = prepared
                .get(&operation.operation_id)
                .ok_or_else(|| invalid("prepared operation gap"))?;
            persist_operation(
                &tx,
                batch,
                operation,
                payload,
                first_sequence + index as u64 + 1,
            )?;
        }
        let receipt_bytes = codec::encode(&expected_receipt, MAX_METADATA_BYTES)?;
        tx.execute(
            "INSERT INTO jj_batches (source_id, digest, receipt, checksum) VALUES (?1, ?2, ?3, ?4)",
            params![
                batch.source_id,
                expected_receipt.digest,
                receipt_bytes,
                codec::checksum(&receipt_bytes)
            ],
        )
        .map_err(|error| sql_error("persist batch receipt", error))?;
        tx.commit()
            .map_err(|error| sql_error("commit capture", error))?;
        Ok(CaptureOutcome::Captured {
            inserted_operations: ordered.len(),
        })
    }
}

fn load_relevant_records(
    conn: &Connection,
    batch: &JjObservationBatch,
) -> Result<BTreeMap<String, JjOperationEvidence>, JournalError> {
    let mut ids: BTreeSet<_> = batch
        .captured_integrated_heads
        .iter()
        .chain(&batch.expected_observed_heads)
        .cloned()
        .collect();
    for operation in &batch.operations {
        ids.insert(operation.operation_id.clone());
        ids.extend(operation.parent_ids.iter().cloned());
    }
    let views = batch
        .operations
        .iter()
        .map(|operation| operation.view_id.clone())
        .collect();
    ids.extend(records::view_representatives(conn, &batch.source_id, &views)?.into_values());
    let incoming: BTreeSet<_> = batch
        .operations
        .iter()
        .map(|operation| operation.operation_id.clone())
        .collect();
    ids.retain(|id| !incoming.contains(id));
    // The partition is defined by the packet, so publishing the packet cannot
    // move newly stored records into its external-boundary budget on retry.
    let mut known = records::load_records(conn, &batch.source_id, &incoming)?;
    known.extend(records::load_records(conn, &batch.source_id, &ids)?);
    Ok(known)
}

fn check_collisions(
    conn: &Connection,
    batch: &JjObservationBatch,
    known: &BTreeMap<String, JjOperationEvidence>,
) -> Result<(), JournalError> {
    let view_ids = batch
        .operations
        .iter()
        .map(|operation| operation.view_id.clone())
        .collect();
    let representatives = records::view_representatives(conn, &batch.source_id, &view_ids)?;
    let mut views = BTreeMap::new();
    for (view, operation) in &representatives {
        let record = known
            .get(operation)
            .ok_or_else(|| invalid("stored view evidence gap"))?;
        if &record.view_id != view {
            return Err(invalid("stored view identity mismatch"));
        }
        views.insert(view.as_str(), record.view_bytes.as_slice());
    }
    for operation in &batch.operations {
        if let Some(previous) = known.get(&operation.operation_id)
            && previous != operation
        {
            return Err(invalid("operation identity collision"));
        }
        if let Some(previous) = views.insert(&operation.view_id, &operation.view_bytes)
            && previous != operation.view_bytes
        {
            return Err(invalid("view identity collision"));
        }
    }
    Ok(())
}

fn prepare_operations(
    batch: &JjObservationBatch,
) -> Result<BTreeMap<String, Vec<u8>>, JournalError> {
    let mut remaining = MAX_JJ_OBSERVATION_BATCH_BYTES;
    let mut prepared = BTreeMap::new();
    for evidence in &batch.operations {
        let record = StoredOperation {
            schema_version: batch.schema_version,
            reader_profile: batch.reader_profile.clone(),
            source_id: batch.source_id.clone(),
            evidence: evidence.clone(),
        };
        let payload = codec::encode(&record, remaining.min(MAX_RECORD_BYTES))?;
        remaining -= payload.len();
        prepared.insert(evidence.operation_id.clone(), payload);
    }
    Ok(prepared)
}

fn persist_operation(
    conn: &Connection,
    batch: &JjObservationBatch,
    evidence: &JjOperationEvidence,
    payload: &[u8],
    sequence: u64,
) -> Result<(), JournalError> {
    conn.execute(
        "INSERT INTO jj_operations (source_id, operation_id, sequence, payload, checksum)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            batch.source_id,
            evidence.operation_id,
            sequence,
            payload,
            codec::checksum(payload)
        ],
    )
    .map_err(|error| sql_error("persist operation", error))?;
    conn.execute(
        "INSERT INTO jj_views (source_id, view_id, operation_id) VALUES (?1, ?2, ?3)
         ON CONFLICT(source_id, view_id) DO NOTHING",
        params![batch.source_id, evidence.view_id, evidence.operation_id],
    )
    .map_err(|error| sql_error("persist view identity", error))?;
    Ok(())
}

fn load_receipt(
    conn: &Connection,
    source: &str,
    digest: &str,
) -> Result<Option<Receipt>, JournalError> {
    let encoded = conn
        .query_row(
            "SELECT length(receipt), substr(receipt, 1, ?3), substr(checksum, 1, 65)
         FROM jj_batches WHERE source_id = ?1 AND digest = ?2",
            params![source, digest, MAX_METADATA_BYTES + 1],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, Vec<u8>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|error| sql_error("read batch receipt", error))?;
    encoded
        .map(|(length, bytes, checksum)| {
            codec::decode(&bytes, length, &checksum, MAX_METADATA_BYTES)
        })
        .transpose()
}
