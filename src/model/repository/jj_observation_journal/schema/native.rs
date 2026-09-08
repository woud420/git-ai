use super::{JournalError, sql_error, unsupported_schema};
use rusqlite::Connection;

const SCHEMA: &str = "
CREATE TABLE jj_native_baselines (
    source_id TEXT NOT NULL,
    baseline_id TEXT NOT NULL,
    record BLOB NOT NULL,
    checksum TEXT NOT NULL,
    PRIMARY KEY (source_id, baseline_id)
);
CREATE TABLE jj_native_sources (
    source_id TEXT PRIMARY KEY NOT NULL,
    baseline_id TEXT NOT NULL,
    state BLOB NOT NULL,
    checksum TEXT NOT NULL,
    FOREIGN KEY (source_id, baseline_id)
        REFERENCES jj_native_baselines(source_id, baseline_id)
);";

pub(super) fn create(conn: &Connection) -> Result<(), JournalError> {
    conn.execute_batch(SCHEMA)
        .map_err(|error| sql_error("create native baseline schema", error))
}

pub(super) fn verify(conn: &Connection) -> Result<(), JournalError> {
    verify_columns(
        conn,
        "jj_native_baselines",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 2),
            ("record", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    verify_columns(
        conn,
        "jj_native_sources",
        &[
            ("source_id", "TEXT", 1),
            ("baseline_id", "TEXT", 0),
            ("state", "BLOB", 0),
            ("checksum", "TEXT", 0),
        ],
    )?;
    verify_foreign_keys(conn, "jj_native_baselines", 0)?;
    verify_foreign_keys(conn, "jj_native_sources", 2)
}

fn verify_columns(
    conn: &Connection,
    table: &str,
    expected: &[(&str, &str, i64); 4],
) -> Result<(), JournalError> {
    let mut statement = conn
        .prepare(
            "SELECT cid, substr(name, 1, 32), substr(type, 1, 8),
             \"notnull\", dflt_value IS NULL, pk, hidden
             FROM pragma_table_xinfo(?1) ORDER BY cid LIMIT 5",
        )
        .map_err(|error| sql_error("prepare native column schema read", error))?;
    let mut rows = statement
        .query([table])
        .map_err(|error| sql_error("read native column schema", error))?;
    let mut count = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read native column schema row", error))?
    {
        let Some((name, kind, primary_key)) = expected.get(count) else {
            return Err(unsupported_schema());
        };
        if row.get::<_, usize>(0).ok() != Some(count)
            || row.get::<_, String>(1).ok().as_deref() != Some(*name)
            || row.get::<_, String>(2).ok().as_deref() != Some(*kind)
            || row.get::<_, bool>(3).ok() != Some(true)
            || row.get::<_, bool>(4).ok() != Some(true)
            || row.get::<_, i64>(5).ok() != Some(*primary_key)
            || row.get::<_, i64>(6).ok() != Some(0)
        {
            return Err(unsupported_schema());
        }
        count += 1;
    }
    if count != expected.len() {
        return Err(unsupported_schema());
    }
    Ok(())
}

fn verify_foreign_keys(
    conn: &Connection,
    table: &str,
    expected_count: usize,
) -> Result<(), JournalError> {
    let mut statement = conn
        .prepare(
            "SELECT id, seq,
             \"table\" = 'jj_native_baselines'
             AND \"from\" = CASE seq WHEN 0 THEN 'source_id' WHEN 1 THEN 'baseline_id' END
             AND \"to\" = CASE seq WHEN 0 THEN 'source_id' WHEN 1 THEN 'baseline_id' END
             AND on_update = 'NO ACTION' AND on_delete = 'NO ACTION' AND match = 'NONE'
             FROM pragma_foreign_key_list(?1) ORDER BY id, seq LIMIT 3",
        )
        .map_err(|error| sql_error("prepare native foreign key schema read", error))?;
    let mut rows = statement
        .query([table])
        .map_err(|error| sql_error("read native foreign key schema", error))?;
    let mut count = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read native foreign key schema row", error))?
    {
        if count >= expected_count
            || row.get::<_, i64>(0).ok() != Some(0)
            || row.get::<_, usize>(1).ok() != Some(count)
            || row.get::<_, bool>(2).ok() != Some(true)
        {
            return Err(unsupported_schema());
        }
        count += 1;
    }
    if count != expected_count {
        return Err(unsupported_schema());
    }
    Ok(())
}
