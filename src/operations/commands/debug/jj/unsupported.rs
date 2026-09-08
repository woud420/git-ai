use super::Error;
use serde_json::Value;
use std::path::Path;

pub(super) fn status(_path: &Path) -> Result<Value, Error> {
    Err(Error::new(
        "unsupported_platform",
        "Native jj observation diagnostics require Linux or macOS.",
    ))
}

pub(super) fn receipt(path: &Path, _source: &str, _admission: &str) -> Result<Value, Error> {
    status(path)
}
