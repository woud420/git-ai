use super::JjObserverError;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

const MAX_PATH_BYTES: usize = 64 * 1024;

fn invalid() -> JjObserverError {
    JjObserverError::new("intent_unavailable", "Observer locator is invalid.")
}

fn validate(path: &Path) -> Result<(), JjObserverError> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) || !path.is_absolute()
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn encode(path: &Path) -> Result<String, JjObserverError> {
    validate(path)?;
    Ok(crate::hex::encode(path.as_os_str().as_bytes()))
}

pub(crate) fn decode(value: &str) -> Result<PathBuf, JjObserverError> {
    if value.is_empty()
        || value.len() > 2 * MAX_PATH_BYTES
        || !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    let digit = |byte| {
        if byte <= b'9' {
            byte - b'0'
        } else {
            byte - b'a' + 10
        }
    };
    let bytes: Vec<_> = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| (digit(pair[0]) << 4) | digit(pair[1]))
        .collect();
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    validate(&path)?;
    Ok(path)
}
