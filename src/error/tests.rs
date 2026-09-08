use super::*;
use std::fmt;

fn assert_display_contract(error: &GitAiError, expected: &str) {
    assert_eq!(error.to_string(), expected);
    // The manual formatter ignores outer flags, even for bare string variants.
    assert_eq!(format!("{error:*>100.3}"), expected);
    assert_eq!(format!("{error:#}"), expected);
    assert!(std::error::Error::source(error).is_none());
}

fn assert_wrapped_error<T: fmt::Display + fmt::Debug>(
    inner: T,
    wrap: impl FnOnce(T) -> GitAiError,
    variant: &str,
    prefix: &str,
) {
    let expected = format!("{prefix}{inner}");
    let debug = format!("{variant}({inner:?})");
    let error = wrap(inner);
    assert_display_contract(&error, &expected);
    assert_eq!(format!("{error:?}"), debug);
}

fn assert_non_lossy_clone(error: GitAiError) {
    let cloned = error.clone();
    assert_eq!(format!("{cloned:?}"), format!("{error:?}"));
    assert_display_contract(&cloned, &error.to_string());
}

fn assert_generic_clone(error: GitAiError) {
    let cloned = error.clone();
    assert!(matches!(&cloned, GitAiError::Generic(message) if message == &error.to_string()));
    assert_display_contract(&cloned, &format!("Generic error: {error}"));
    assert_non_lossy_clone(cloned);
}

#[test]
fn test_error_display_io_error() {
    let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    assert_wrapped_error(inner, GitAiError::from, "IoError", "IO error: ");
}

#[test]
fn test_error_display_git_cli_error_with_code() {
    let error = GitAiError::GitCliError {
        code: Some(128),
        stderr: "fatal: not a git repository".into(),
        args: vec!["git".into(), "status".into()],
    };
    assert_display_contract(
        &error,
        "Git CLI (git status) failed with exit code 128: fatal: not a git repository",
    );
    assert_eq!(
        format!("{error:?}"),
        r#"GitCliError { code: Some(128), stderr: "fatal: not a git repository", args: ["git", "status"] }"#
    );
}

#[test]
fn test_error_display_git_cli_error_without_code() {
    let error = GitAiError::GitCliError {
        code: None,
        stderr: "command terminated".into(),
        args: vec!["git".into(), "push".into()],
    };
    assert_display_contract(&error, "Git CLI (git push) failed: command terminated");
    assert_eq!(
        format!("{error:?}"),
        r#"GitCliError { code: None, stderr: "command terminated", args: ["git", "push"] }"#
    );
}

#[test]
fn test_error_display_json_error() {
    let inner = serde_json::from_str::<serde_json::Value>("{invalid json").unwrap_err();
    assert_wrapped_error(inner, GitAiError::from, "JsonError", "JSON error: ");
}

#[test]
fn test_error_display_utf8_error() {
    let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
    let inner = std::str::from_utf8(&invalid_utf8).unwrap_err();
    assert_wrapped_error(inner, GitAiError::from, "Utf8Error", "UTF-8 error: ");
}

#[test]
fn test_error_display_from_utf8_error() {
    let inner = String::from_utf8(vec![0xFF, 0xFE, 0xFD]).unwrap_err();
    assert_wrapped_error(
        inner,
        GitAiError::from,
        "FromUtf8Error",
        "From UTF-8 error: ",
    );
}

#[test]
fn test_error_display_preset_error() {
    assert_wrapped_error(
        "invalid preset configuration".to_string(),
        GitAiError::PresetError,
        "PresetError",
        "",
    );
}

#[test]
fn test_error_display_sqlite_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("error.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    let inner = conn.execute("INVALID SQL", []).unwrap_err();
    assert_wrapped_error(inner, GitAiError::from, "SqliteError", "SQLite error: ");
}

#[test]
fn test_error_display_generic() {
    assert_wrapped_error(
        "custom error message".to_string(),
        GitAiError::Generic,
        "Generic",
        "Generic error: ",
    );
}

#[test]
fn test_error_display_gix_error() {
    assert_wrapped_error(
        "gix operation failed".to_string(),
        GitAiError::GixError,
        "GixError",
        "Gix error: ",
    );
}

#[test]
fn test_error_clone_io_error() {
    for inner in [
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied"),
        std::io::Error::from_raw_os_error(13),
    ] {
        let (kind, message) = (inner.kind(), inner.to_string());
        let error = GitAiError::from(inner);
        let cloned = error.clone();
        let GitAiError::IoError(inner) = &cloned else {
            panic!("Expected IoError")
        };
        assert_eq!(inner.kind(), kind);
        assert_eq!(inner.to_string(), message);
        assert_eq!(inner.raw_os_error(), None);
        assert_display_contract(&cloned, &error.to_string());
    }
}

#[test]
fn test_error_clone_git_cli_error() {
    for code in [Some(1), None] {
        assert_non_lossy_clone(GitAiError::GitCliError {
            code,
            stderr: "error message".into(),
            args: vec!["git".into(), "commit".into()],
        });
    }
}

#[test]
fn test_error_clone_utf8_error() {
    let invalid_utf8 = vec![0xFF];
    assert_non_lossy_clone(std::str::from_utf8(&invalid_utf8).unwrap_err().into());
}

#[test]
fn test_error_clone_from_utf8_error() {
    assert_non_lossy_clone(String::from_utf8(vec![0xFF]).unwrap_err().into());
}

#[test]
fn test_error_clone_preset_error() {
    assert_non_lossy_clone(GitAiError::PresetError("preset error".into()));
}

#[test]
fn test_error_clone_generic() {
    assert_non_lossy_clone(GitAiError::Generic("generic".into()));
}

#[test]
fn test_error_clone_json_converts_to_generic() {
    assert_generic_clone(
        serde_json::from_str::<serde_json::Value>("{bad}")
            .unwrap_err()
            .into(),
    );
}

#[test]
fn test_error_clone_sqlite_converts_to_generic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("error.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    assert_generic_clone(conn.execute("BAD SQL", []).unwrap_err().into());
}

#[test]
fn test_error_clone_gix_converts_to_generic() {
    assert_generic_clone(GitAiError::GixError("gix error".into()));
}

#[test]
fn test_error_is_std_error() {
    assert_display_contract(&GitAiError::Generic("test".into()), "Generic error: test");
}

#[test]
fn test_error_debug_trait() {
    let error = GitAiError::Generic("debug test".into());
    assert_eq!(format!("{error:?}"), "Generic(\"debug test\")");
    assert_eq!(format!("{error:#?}"), "Generic(\n    \"debug test\",\n)");
}

#[test]
fn test_error_persistence_display_delegates_to_inner() {
    let inner =
        crate::model::repository::error::PersistenceError::LockPoisoned { what: "notes-db" };
    assert_wrapped_error(inner, GitAiError::from, "Persistence", "");
}

#[test]
fn test_error_persistence_clone_is_non_lossy() {
    let inner = crate::model::repository::error::PersistenceError::Sqlite {
        db: "metrics",
        operation: "insert",
        code: Some(rusqlite::ffi::ErrorCode::DatabaseBusy),
        message: "busy".into(),
    };
    assert_non_lossy_clone(inner.into());
}

#[test]
fn test_error_api_display_and_clone() {
    for status in [Some(401), None, Some(503)] {
        let inner = crate::clients::api::error::ApiError {
            operation: "upload",
            status,
            message: "unavailable".into(),
        };
        assert_non_lossy_clone(inner.clone().into());
        assert_wrapped_error(inner, GitAiError::from, "Api", "");
    }
}
