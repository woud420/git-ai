//! Value extraction from Cube row objects (`Map<String, Value>`) and
//! SQLite `ValueRef` → JSON conversion.

use serde_json::{Map, Value};

/// Look up a cube member (`<cube>.<field>`) in a row.
pub(super) fn member<'a>(row: &'a Value, cube: &str, field: &str) -> Option<&'a Value> {
    row.get(format!("{}.{}", cube, field))
}

/// Get a cube member (`<cube>.<field>`) from a row as an owned String.
pub(super) fn member_str(row: &Value, cube: &str, field: &str) -> Option<String> {
    match member(row, cube, field) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

/// Get a numeric cube member. Cube returns numbers as strings, so parse both.
pub(super) fn member_int(row: &Value, cube: &str, field: &str) -> Option<i64> {
    match member(row, cube, field) {
        Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|f| f as i64),
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

/// Get a numeric cube member as f64 (e.g. cost). Cube returns numbers as
/// strings, so parse both.
pub(super) fn member_float(row: &Value, cube: &str, field: &str) -> Option<f64> {
    match member(row, cube, field) {
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

/// Get a time-typed cube member (ISO8601 string) as unix seconds.
pub(super) fn member_time(row: &Value, cube: &str, field: &str) -> Option<i64> {
    parse_cube_time(member(row, cube, field)?.as_str()?)
}

/// Parse a Cube time value (RFC3339 or `YYYY-MM-DDTHH:MM:SS[.fff]`) to unix secs.
pub(super) fn parse_cube_time(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.timestamp());
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S%.f",
    ] {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt.and_utc().timestamp());
        }
    }
    None
}

pub(super) fn value_ref_to_json(v: rusqlite::types::ValueRef) -> Value {
    use rusqlite::types::ValueRef;
    use serde_json::json;
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
    }
}

/// Build a JSON object from a SQLite row, keyed by the given (select-order)
/// column names.
pub(super) fn row_to_json_object(
    row: &rusqlite::Row,
    cols: &[&str],
) -> Result<Map<String, Value>, rusqlite::Error> {
    let mut obj = Map::new();
    for (idx, name) in cols.iter().enumerate() {
        obj.insert((*name).to_string(), value_ref_to_json(row.get_ref(idx)?));
    }
    Ok(obj)
}

pub(super) fn value_ref_to_string(v: rusqlite::types::ValueRef) -> String {
    use rusqlite::types::ValueRef;
    match v {
        ValueRef::Null => String::new(),
        ValueRef::Integer(i) => i.to_string(),
        ValueRef::Real(f) => f.to_string(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned(),
    }
}
