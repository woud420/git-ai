use super::super::{native_baseline, registration_records};
use super::*;
use registration_records::read as records;
use rusqlite::Connection;

const SOURCE_ROOT_GUARD_SQL: &str = "SELECT 1 FROM jj_native_registrations
    WHERE source_root_key COLLATE BINARY = ?1 LIMIT 1";
const WORKSPACE_LOCATOR_GUARD_SQL: &str = "SELECT 1 FROM jj_native_workspaces
    WHERE locator_key COLLATE BINARY = ?1 LIMIT 1";
const SOURCE_WORKSPACE_EXISTS_SQL: &str = "SELECT 1 FROM jj_native_workspaces
    WHERE source_id COLLATE BINARY = ?1 LIMIT 1";
const SOURCE_WORKSPACE_COUNT_SQL: &str = "SELECT 1 FROM jj_native_workspaces
    WHERE source_id COLLATE BINARY = ?1 LIMIT 2";
const NATIVE_STATE_COUNT_SQL: &str = "SELECT 1 FROM jj_native_sources
    WHERE source_id COLLATE BINARY = ?1 LIMIT 2";
const NATIVE_BASELINE_EXISTS_SQL: &str = "SELECT 1 FROM jj_native_baselines
    WHERE source_id COLLATE BINARY = ?1 LIMIT 1";

fn exists(conn: &Connection, sql: &str, key: &str) -> Result<bool, JournalError> {
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| sql_error("prepare native registration presence read", error))?;
    let mut rows = statement
        .query([key])
        .map_err(|error| sql_error("read native registration presence", error))?;
    Ok(rows
        .next()
        .map_err(|error| sql_error("read native registration presence row", error))?
        .is_some())
}

pub(super) fn guards_occupied(
    conn: &Connection,
    source_root_key: &str,
    locator_key: &str,
) -> Result<bool, JournalError> {
    Ok(exists(conn, SOURCE_ROOT_GUARD_SQL, source_root_key)?
        || exists(conn, WORKSPACE_LOCATOR_GUARD_SQL, locator_key)?)
}

fn unregistered_rows_present(conn: &Connection, source: &str) -> Result<bool, JournalError> {
    Ok(exists(conn, NATIVE_STATE_COUNT_SQL, source)?
        || exists(conn, NATIVE_BASELINE_EXISTS_SQL, source)?
        || exists(conn, SOURCE_WORKSPACE_EXISTS_SQL, source)?)
}

pub(super) fn require_absent(conn: &Connection, source: &str) -> Result<(), JournalError> {
    if records::has_one(conn, records::REGISTRATION_COUNT_SQL, [source])?
        || unregistered_rows_present(conn, source)?
    {
        return Err(invalid(
            "native registration installation requires absent source",
        ));
    }
    Ok(())
}

pub(super) fn require_initial_workspace(
    conn: &Connection,
    source: &str,
) -> Result<(), JournalError> {
    if !records::has_one(conn, SOURCE_WORKSPACE_COUNT_SQL, [source])? {
        return Err(invalid("native registration installation workspace gap"));
    }
    Ok(())
}

pub(super) fn snapshot(
    conn: &Connection,
    source: &str,
    workspace: &str,
    budget: &mut ReadBudget,
) -> Result<Option<StoredRegistrationSnapshot>, JournalError> {
    let Some(registration) = records::registration_stored(conn, source, budget)? else {
        if unregistered_rows_present(conn, source)? {
            return Err(invalid(
                "native registration source is incomplete or unregistered",
            ));
        }
        return Ok(None);
    };
    let original_workspace = records::workspace_stored(
        conn,
        source,
        &registration.record.initial_workspace_name,
        budget,
    )?
    .ok_or_else(|| invalid("native registration original workspace gap"))?;
    validate_workspace_scope(&registration.record, &original_workspace.record)?;
    if original_workspace.record.attachment_id != registration.record.initial_attachment_id
        || original_workspace.checksum != registration.record.initial_workspace_record_id
    {
        return Err(invalid(
            "native registration original workspace receipt mismatch",
        ));
    }
    let selected_workspace = if workspace == original_workspace.record.workspace_name {
        None
    } else {
        let selected = records::workspace_stored(conn, source, workspace, budget)?
            .ok_or_else(|| invalid("native registration selected workspace gap"))?;
        validate_workspace_scope(&registration.record, &selected.record)?;
        Some(selected)
    };
    // The shared native reader assumes its schema PK; guard a damaged live schema
    // before selecting its state payload without changing standalone behavior.
    if !records::has_one(conn, NATIVE_STATE_COUNT_SQL, [source])? {
        return Err(invalid("native registration baseline state gap"));
    }
    let native = native_baseline::read::snapshot(conn, source, budget)?
        .ok_or_else(|| invalid("native registration baseline gap"))?;
    let record = &registration.record;
    if native.state.source_id != record.source_id
        || native.state.reader_profile != record.reader_profile
        || native.state.baseline_id != record.baseline_id
        || native.state.generation != record.baseline_generation
    {
        return Err(invalid("native registration baseline scope mismatch"));
    }
    Ok(Some(StoredRegistrationSnapshot {
        registration,
        original_workspace,
        selected_workspace,
        native,
    }))
}

fn validate_workspace_scope(
    registration: &RegistrationRecord,
    workspace: &WorkspaceRecord,
) -> Result<(), JournalError> {
    if workspace.source_id != registration.source_id
        || workspace.reader_profile != registration.reader_profile
        || workspace.baseline_id != registration.baseline_id
        || workspace.baseline_generation != registration.baseline_generation
        || workspace.seal_digest != registration.seal_digest
        || workspace.locator.platform != registration.source_binding.platform
    {
        return Err(invalid("native registration workspace scope mismatch"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "read_tests.rs"]
mod tests;
