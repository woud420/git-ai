use super::Error;
use crate::operations::jj::admission::NativeAdmissionExpectation;
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

pub(super) fn initialize(path: &Path) -> Result<Value, Error> {
    status(path)
}

pub(super) fn capture(
    path: &Path,
    _expected: NativeAdmissionExpectation<'_>,
) -> Result<Value, Error> {
    status(path)
}

pub(super) fn observe(request: super::args::ObserveArgs<'_>) -> Result<Value, Error> {
    let super::args::ObserveArgs {
        journal,
        expected,
        workspace,
        attachment,
        attempts,
        interval_ms,
    } = request;
    let _ = (expected, workspace, attachment, attempts, interval_ms);
    status(journal)
}
