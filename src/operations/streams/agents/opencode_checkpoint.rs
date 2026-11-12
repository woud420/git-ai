use crate::model::checkpoint_request::{
    StreamFormat as CheckpointStreamFormat, StreamSource as CheckpointStreamSource,
};
use crate::model::stream_types::StreamError;
use crate::operations::streams::agent::{
    checkpoint_stream_denied, validate_checkpoint_stream_claim, validate_checkpoint_stream_file,
};
use crate::operations::streams::sweep::DiscoveredSession;
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub fn open_sqlite_readonly(path: &Path) -> Result<Connection, StreamError> {
    let connection = crate::model::repository::sqlite::open_with_flags_and_memory_limits(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|error| StreamError::Fatal {
        message: format!(
            "Failed to open OpenCode database {}: {}",
            path.display(),
            error
        ),
    })?;

    connection
        .execute_batch("PRAGMA busy_timeout = 5000;")
        .map_err(|error| StreamError::Fatal {
            message: format!("Failed to set PRAGMAs: {}", error),
        })?;

    Ok(connection)
}

pub(super) fn validate_checkpoint_stream(
    source: &CheckpointStreamSource,
) -> Result<DiscoveredSession, StreamError> {
    validate_checkpoint_stream_claim(source, "opencode", CheckpointStreamFormat::OpenCodeSqlite)?;
    let database_path = storage_path()
        .as_deref()
        .and_then(database_path)
        .ok_or_else(checkpoint_stream_denied)?;
    let trusted_root = database_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(checkpoint_stream_denied)?;
    let mut host_source = source.clone();
    host_source.path = database_path;

    validate_checkpoint_stream_file(
        &host_source,
        "opencode",
        CheckpointStreamFormat::OpenCodeSqlite,
        vec![trusted_root],
        |path| {
            let connection = open_sqlite_readonly(path).ok()?;
            let parent = super::opencode::schema::parent(&connection, &source.external_session_id)?;
            Some((source.external_session_id.clone(), parent))
        },
    )
}

pub(crate) fn storage_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("GIT_AI_OPENCODE_STORAGE_PATH") {
        return Some(PathBuf::from(path));
    }
    let home_var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    let home = std::env::var_os(home_var)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(dirs::home_dir);
    data_path(std::env::var_os("XDG_DATA_HOME"), home)
}

fn data_path(xdg: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(xdg) = xdg.filter(|value| !value.is_empty()) {
        return Some(PathBuf::from(xdg).join("opencode"));
    }
    home.map(|home| home.join(".local/share/opencode"))
}

pub(crate) fn database_path(storage_path: &Path) -> Option<PathBuf> {
    if storage_path.file_name().and_then(|name| name.to_str()) == Some("opencode.db")
        && storage_path.is_file()
    {
        return Some(storage_path.to_path_buf());
    }
    let direct = storage_path.join("opencode.db");
    if direct.is_file() {
        return Some(direct);
    }
    if storage_path.file_name().and_then(|name| name.to_str()) == Some("storage") {
        let sibling = storage_path.parent()?.join("opencode.db");
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn empty_xdg_uses_home_and_nonempty_xdg_takes_precedence() {
        use std::ffi::OsString;
        use std::path::PathBuf;
        let home = PathBuf::from("home");
        assert_eq!(
            super::data_path(Some(OsString::new()), Some(home.clone())),
            Some(home.join(".local/share/opencode"))
        );
        assert_eq!(
            super::data_path(Some(OsString::from("data")), Some(home)),
            Some(PathBuf::from("data/opencode"))
        );
    }
}
