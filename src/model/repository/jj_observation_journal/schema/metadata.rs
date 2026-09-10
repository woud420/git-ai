use super::{JournalError, sql_error, unsupported_schema};
use rusqlite::{Connection, params};

pub(super) fn verify_columns(
    conn: &Connection,
    table: &str,
    expected: &[(&str, &str, i64)],
) -> Result<(), JournalError> {
    if expected.len() > 6 {
        return Err(unsupported_schema());
    }
    let mut statement = conn
        .prepare(
            "SELECT cid, substr(name, 1, 32), substr(type, 1, 8),
             \"notnull\", dflt_value IS NULL, pk, hidden
             FROM pragma_table_xinfo(?1) ORDER BY cid LIMIT ?2",
        )
        .map_err(|error| sql_error("prepare native column schema read", error))?;
    let mut rows = statement
        .query(params![table, expected.len() + 1])
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

pub(super) fn verify_foreign_key(
    conn: &Connection,
    table: &str,
    parent: &str,
    columns: &[(&str, &str)],
) -> Result<(), JournalError> {
    if columns.len() > 2 {
        return Err(unsupported_schema());
    }
    let first = columns.first().copied();
    let second = columns.get(1).copied();
    let mut statement = conn
        .prepare(
            "SELECT id, seq,
             \"table\" = ?2
             AND \"from\" = CASE seq WHEN 0 THEN ?3 WHEN 1 THEN ?5 END
             AND \"to\" = CASE seq WHEN 0 THEN ?4 WHEN 1 THEN ?6 END
             AND on_update = 'NO ACTION' AND on_delete = 'NO ACTION' AND match = 'NONE'
             FROM pragma_foreign_key_list(?1) ORDER BY id, seq LIMIT ?7",
        )
        .map_err(|error| sql_error("prepare native foreign key schema read", error))?;
    let mut rows = statement
        .query(params![
            table,
            parent,
            first.map(|pair| pair.0),
            first.map(|pair| pair.1),
            second.map(|pair| pair.0),
            second.map(|pair| pair.1),
            columns.len() + 1,
        ])
        .map_err(|error| sql_error("read native foreign key schema", error))?;
    let mut count = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read native foreign key schema row", error))?
    {
        if count >= columns.len()
            || row.get::<_, i64>(0).ok() != Some(0)
            || row.get::<_, usize>(1).ok() != Some(count)
            || row.get::<_, bool>(2).ok() != Some(true)
        {
            return Err(unsupported_schema());
        }
        count += 1;
    }
    if count != columns.len() {
        return Err(unsupported_schema());
    }
    Ok(())
}
