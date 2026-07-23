use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Recover a poisoned mutex by taking its inner guard anyway.
///
/// A panic while holding the lock cannot corrupt an in-memory connection or
/// counter the way it could corrupt on-disk state, so callers that just need
/// continued access (as opposed to `PersistenceError::LockPoisoned`'s
/// fail-closed sites) recover rather than propagate.
pub fn poisoned_lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Negative SQLite cache_size values are kibibytes. Keep each connection's
/// page cache capped at 2 MiB unless a caller deliberately changes it later.
pub const MEMORY_LIMIT_CACHE_SIZE_KIB: i32 = -2000;

pub fn apply_memory_limits(conn: &Connection) -> rusqlite::Result<()> {
    conn.pragma_update(None, "cache_size", MEMORY_LIMIT_CACHE_SIZE_KIB)
}

pub fn open_with_memory_limits(path: impl AsRef<Path>) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    apply_memory_limits(&conn)?;
    Ok(conn)
}

pub fn open_writable_with_memory_limits(path: impl AsRef<Path>) -> rusqlite::Result<Connection> {
    let conn = open_with_memory_limits(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA temp_store=MEMORY;",
    )?;
    Ok(conn)
}

pub fn open_with_flags_and_memory_limits(
    path: impl AsRef<Path>,
    flags: OpenFlags,
) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(path, flags)?;
    apply_memory_limits(&conn)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_with_memory_limits_sets_cache_size() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("limited.db");

        let conn = open_with_memory_limits(&db_path).unwrap();

        let cache_size: i32 = conn
            .pragma_query_value(None, "cache_size", |row| row.get(0))
            .unwrap();
        assert_eq!(cache_size, MEMORY_LIMIT_CACHE_SIZE_KIB);
    }

    #[test]
    fn open_writable_with_memory_limits_sets_write_policy() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("writable.db");

        let conn = open_writable_with_memory_limits(&db_path).unwrap();

        let cache_size: i32 = conn
            .pragma_query_value(None, "cache_size", |row| row.get(0))
            .unwrap();
        let journal_mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        let synchronous: i32 = conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        let temp_store: i32 = conn
            .pragma_query_value(None, "temp_store", |row| row.get(0))
            .unwrap();

        assert_eq!(cache_size, MEMORY_LIMIT_CACHE_SIZE_KIB);
        assert_eq!(journal_mode, "wal");
        assert_eq!(synchronous, 1);
        assert_eq!(temp_store, 2);
    }

    #[test]
    fn open_with_flags_and_memory_limits_sets_cache_size() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("limited-readonly.db");
        drop(open_with_memory_limits(&db_path).unwrap());

        let conn =
            open_with_flags_and_memory_limits(&db_path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();

        let cache_size: i32 = conn
            .pragma_query_value(None, "cache_size", |row| row.get(0))
            .unwrap();
        assert_eq!(cache_size, MEMORY_LIMIT_CACHE_SIZE_KIB);
    }
}
