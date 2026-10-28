use super::super::{JjObservationJournal, JournalError, ReadBudget, invalid, schema, sql_error};
use super::{StoredAdmissionSnapshot, payload, read};
use crate::model::jj_observation::validate_source;
use rusqlite::Connection;

pub(crate) enum ReplaySelection<'a> {
    Latest,
    Receipt(&'a str),
    Generation(u64),
}

impl JjObservationJournal {
    /// Epoch and selected records belong to one fresh snapshot on this connection.
    /// No snapshot is retained between bounded replay jobs.
    pub(crate) fn read_replay_snapshot(
        &self,
        source: &str,
        workspace: &str,
        selection: ReplaySelection<'_>,
        reads: &mut ReadBudget,
    ) -> Result<(u64, StoredAdmissionSnapshot), JournalError> {
        validate_source(source)?;
        require_connection_residency(&self.conn)?;
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|error| sql_error("begin native replay snapshot", error))?;
        // Establish the actual main read snapshot before reading its epoch, and
        // reject changed indexes/schema before any generation lookup can scan.
        schema::verify_in_snapshot(&tx)?;
        let epoch = data_version(&tx)?;
        let generation_id;
        let requested = match selection {
            ReplaySelection::Latest => None,
            ReplaySelection::Receipt(id) => {
                validate_source(id)?;
                Some(id)
            }
            ReplaySelection::Generation(generation) => {
                let mut query = tx
                    .prepare(read::GENERATION_IDS_SQL)
                    .map_err(|error| sql_error("prepare replay generation", error))?;
                let mut rows = query
                    .query(rusqlite::params![source, generation])
                    .map_err(|error| sql_error("select replay generation", error))?;
                let row = rows
                    .next()
                    .map_err(|error| sql_error("read replay generation", error))?
                    .ok_or_else(|| invalid("native replay generation gap"))?;
                generation_id = payload::text(row, 0)?;
                validate_source(&generation_id)?;
                if rows
                    .next()
                    .map_err(|error| sql_error("read replay cardinality", error))?
                    .is_some()
                {
                    return Err(invalid("native replay generation conflict"));
                }
                Some(generation_id.as_str())
            }
        };
        let snapshot = read::snapshot(&tx, source, Some(workspace), requested, reads)?;
        Ok((epoch, snapshot))
    }
}

pub(super) fn data_version(conn: &Connection) -> Result<u64, JournalError> {
    conn.pragma_query_value(Some(rusqlite::DatabaseName::Main), "data_version", |row| {
        row.get(0)
    })
    .map_err(|error| sql_error("read replay connection epoch", error))
}

pub(super) fn guard_cached_write(conn: &Connection, epoch: u64) -> Result<(), JournalError> {
    require_connection_residency(conn)?;
    if data_version(conn)? != epoch {
        return Err(invalid("native replay connection changed"));
    }
    schema::verify_in_snapshot(conn)?;
    // Own commits do not advance this connection's data_version. A trigger could
    // mutate a previously verified dependency while leaving the new packet valid.
    // This private connection never creates TEMP triggers or accepts arbitrary SQL.
    let triggers: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM main.sqlite_master WHERE type='trigger'
          AND tbl_name IN ('jj_native_admissions','jj_native_admission_states',
           'jj_native_baselines','jj_native_sources','jj_native_registrations','jj_native_workspaces')
         UNION ALL SELECT 1 FROM temp.sqlite_master WHERE type='trigger')",
        [], |row| row.get(0),
    ).map_err(|error| sql_error("check replay write triggers", error))?;
    if triggers {
        return Err(invalid("cached native admission refuses triggers"));
    }
    Ok(())
}

fn require_connection_residency(conn: &Connection) -> Result<(), JournalError> {
    let mut moved: std::ffi::c_int = 0;
    // SQLite's VFS compares the live database's identity without opening an
    // unrelated database descriptor (which could release process-wide locks).
    // This detects ordinary rename/unlink, not arbitrary raw-file rollback.
    let result = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            conn.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            (&mut moved as *mut std::ffi::c_int).cast(),
        )
    };
    if result != rusqlite::ffi::SQLITE_OK || moved != 0 {
        return Err(invalid(
            "native replay database identity is unavailable or moved",
        ));
    }
    Ok(())
}
