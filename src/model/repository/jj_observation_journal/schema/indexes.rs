use super::{JournalError, sql_error, unsupported_schema};
use rusqlite::{Connection, params};

const MAX_INDEX_NAME_BYTES: usize = 256;

/// The first expected key is the table primary key; others are unique guards.
/// This verifies key-index collation, not each column's default collation.
pub(super) fn verify(
    conn: &Connection,
    table: &str,
    expected: &[&[&str]],
) -> Result<(), JournalError> {
    if expected.is_empty()
        || expected.len() > 3
        || expected.iter().any(|key| key.is_empty() || key.len() > 2)
    {
        return Err(unsupported_schema());
    }
    let mut statement = conn
        .prepare(
            "SELECT CASE WHEN typeof(name) = 'text' AND length(CAST(name AS BLOB)) <= ?2
                         THEN name ELSE NULL END,
             \"unique\", CASE origin WHEN 'pk' THEN 0 WHEN 'u' THEN 1 WHEN 'c' THEN 1 END,
             partial
             FROM pragma_index_list(?1) ORDER BY seq LIMIT ?3",
        )
        .map_err(|error| sql_error("prepare registration index schema read", error))?;
    let mut rows = statement
        .query(params![table, MAX_INDEX_NAME_BYTES, expected.len() + 1])
        .map_err(|error| sql_error("read registration index schema", error))?;
    let mut seen = [false; 3];
    let mut count = 0;
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read registration index schema row", error))?
    {
        if count >= expected.len()
            || row.get::<_, bool>(1).ok() != Some(true)
            || row.get::<_, bool>(3).ok() != Some(false)
        {
            return Err(unsupported_schema());
        }
        let name = row
            .get::<_, Option<String>>(0)
            .ok()
            .flatten()
            .filter(|name| !name.as_bytes().contains(&0))
            .ok_or_else(unsupported_schema)?;
        let kind = row
            .get::<_, Option<u8>>(2)
            .ok()
            .flatten()
            .ok_or_else(unsupported_schema)?;
        let key_limit = if kind == 0 {
            expected[0].len()
        } else {
            expected
                .iter()
                .skip(1)
                .map(|key| key.len())
                .max()
                .unwrap_or(0)
        };
        let columns = key_columns(conn, &name, key_limit)?;
        let Some(position) = expected.iter().enumerate().find_map(|(position, key)| {
            (((kind == 0) == (position == 0))
                && columns.iter().map(String::as_str).eq(key.iter().copied()))
            .then_some(position)
        }) else {
            return Err(unsupported_schema());
        };
        if seen[position] {
            return Err(unsupported_schema());
        }
        seen[position] = true;
        count += 1;
    }
    if count != expected.len() || seen[..expected.len()].iter().any(|seen| !seen) {
        return Err(unsupported_schema());
    }
    Ok(())
}

fn key_columns(conn: &Connection, index: &str, limit: usize) -> Result<Vec<String>, JournalError> {
    let mut statement = conn
        .prepare(
            "SELECT seqno, cid,
             CASE WHEN typeof(name) = 'text' AND length(CAST(name AS BLOB)) <= 32
                  THEN name ELSE NULL END,
             \"desc\", coll = 'BINARY'
             FROM pragma_index_xinfo(?1) WHERE \"key\" = 1 ORDER BY seqno LIMIT ?2",
        )
        .map_err(|error| sql_error("prepare registration index key read", error))?;
    let mut rows = statement
        .query(params![index, limit + 1])
        .map_err(|error| sql_error("read registration index keys", error))?;
    let mut columns = Vec::with_capacity(limit);
    while let Some(row) = rows
        .next()
        .map_err(|error| sql_error("read registration index key row", error))?
    {
        if columns.len() >= limit
            || row.get::<_, usize>(0).ok() != Some(columns.len())
            || row.get::<_, i64>(1).ok().filter(|cid| *cid >= 0).is_none()
            || row.get::<_, bool>(3).ok() != Some(false)
            || row.get::<_, bool>(4).ok() != Some(true)
        {
            return Err(unsupported_schema());
        }
        columns.push(
            row.get::<_, Option<String>>(2)
                .ok()
                .flatten()
                .ok_or_else(unsupported_schema)?,
        );
    }
    Ok(columns)
}
