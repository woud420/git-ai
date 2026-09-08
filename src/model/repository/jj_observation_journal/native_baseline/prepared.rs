use super::super::{JournalError, codec, invalid, sql_error};
use super::types::{MAX_BASELINE_BYTES, MAX_STATE_BYTES, NativeBaselineState, Request};
use rusqlite::{Transaction, params};

pub(super) struct PreparedBaseline<'a> {
    pub(super) request: Request<'a>,
    pub(super) state: NativeBaselineState,
    record_bytes: Vec<u8>,
    state_bytes: Vec<u8>,
}

impl<'a> PreparedBaseline<'a> {
    pub(super) fn new(request: Request<'a>) -> Result<Self, JournalError> {
        // Only bounded vectors of references are canonicalized; raw evidence is not cloned.
        let record_bytes = codec::encode(&request, MAX_BASELINE_BYTES)?;
        let baseline_id = codec::checksum(&record_bytes);
        let state = request.state(baseline_id);
        let state_bytes = codec::encode(&state, MAX_STATE_BYTES)?;
        Ok(Self {
            request,
            state,
            record_bytes,
            state_bytes,
        })
    }

    pub(super) fn insert_into(
        self,
        tx: &Transaction<'_>,
    ) -> Result<(Request<'a>, NativeBaselineState), JournalError> {
        let Self {
            request,
            state,
            record_bytes,
            state_bytes,
        } = self;
        let source = request.source_id;
        let inserted = tx.execute(
            "INSERT INTO jj_native_baselines(source_id, baseline_id, record, checksum) VALUES (?1, ?2, ?3, ?4)",
            params![source, state.baseline_id, record_bytes, state.baseline_id],
        ).map_err(|error| sql_error("persist native baseline record", error))?;
        if inserted != 1 {
            return Err(invalid(
                "native baseline record insert did not persist one row",
            ));
        }
        let inserted = tx.execute(
            "INSERT INTO jj_native_sources(source_id, baseline_id, state, checksum) VALUES (?1, ?2, ?3, ?4)",
            params![source, state.baseline_id, state_bytes, codec::checksum(&state_bytes)],
        ).map_err(|error| sql_error("persist native baseline state", error))?;
        if inserted != 1 {
            return Err(invalid(
                "native baseline state insert did not persist one row",
            ));
        }
        drop(record_bytes);
        drop(state_bytes);
        Ok((request, state))
    }
}

#[cfg(test)]
#[path = "prepared_tests.rs"]
mod tests;
