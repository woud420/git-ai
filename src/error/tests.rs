use super::*;

#[test]
fn test_error_display_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let err = GitAiError::from(io_err);
    let display = format!("{}", err);
    assert!(display.contains("IO error"));
    assert!(display.contains("file not found"));
}

#[test]
fn test_error_display_git_cli_error_with_code() {
    let err = GitAiError::GitCliError {
        code: Some(128),
        stderr: "fatal: not a git repository".to_string(),
        args: vec!["git".to_string(), "status".to_string()],
    };
    let display = format!("{}", err);
    assert!(display.contains("128"));
    assert!(display.contains("fatal: not a git repository"));
    assert!(display.contains("git status"));
}

#[test]
fn test_error_display_git_cli_error_without_code() {
    let err = GitAiError::GitCliError {
        code: None,
        stderr: "command terminated".to_string(),
        args: vec!["git".to_string(), "push".to_string()],
    };
    let display = format!("{}", err);
    assert!(display.contains("Git CLI"));
    assert!(display.contains("command terminated"));
    assert!(display.contains("git push"));
}

#[test]
fn test_error_display_json_error() {
    let json_str = "{invalid json";
    let json_err = serde_json::from_str::<serde_json::Value>(json_str).unwrap_err();
    let err = GitAiError::from(json_err);
    let display = format!("{}", err);
    assert!(display.contains("JSON error"));
}

#[test]
fn test_error_display_utf8_error() {
    let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
    let utf8_err = std::str::from_utf8(&invalid_utf8).unwrap_err();
    let err = GitAiError::from(utf8_err);
    let display = format!("{}", err);
    assert!(display.contains("UTF-8 error"));
}

#[test]
fn test_error_display_from_utf8_error() {
    let invalid_utf8 = vec![0xFF, 0xFE, 0xFD];
    let from_utf8_err = String::from_utf8(invalid_utf8).unwrap_err();
    let err = GitAiError::from(from_utf8_err);
    let display = format!("{}", err);
    assert!(display.contains("From UTF-8 error"));
}

#[test]
fn test_error_display_preset_error() {
    let err = GitAiError::PresetError("invalid preset configuration".to_string());
    let display = format!("{}", err);
    assert_eq!(display, "invalid preset configuration");
}

#[test]
fn test_error_display_sqlite_error() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("error.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    let sql_err = conn.execute("INVALID SQL", []).unwrap_err();
    let err = GitAiError::from(sql_err);
    let display = format!("{}", err);
    assert!(display.contains("SQLite error"));
}

#[test]
fn test_error_display_generic() {
    let err = GitAiError::Generic("custom error message".to_string());
    let display = format!("{}", err);
    assert!(display.contains("Generic error"));
    assert!(display.contains("custom error message"));
}

#[test]
fn test_error_display_gix_error() {
    let err = GitAiError::GixError("gix operation failed".to_string());
    let display = format!("{}", err);
    assert!(display.contains("Gix error"));
    assert!(display.contains("gix operation failed"));
}

#[test]
fn test_error_clone_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
    let err = GitAiError::from(io_err);
    let cloned = err.clone();
    assert!(matches!(cloned, GitAiError::IoError(_)));
    let display = format!("{}", cloned);
    assert!(display.contains("access denied"));
}

#[test]
fn test_error_clone_git_cli_error() {
    let err = GitAiError::GitCliError {
        code: Some(1),
        stderr: "error message".to_string(),
        args: vec!["git".to_string(), "commit".to_string()],
    };
    let cloned = err.clone();
    match cloned {
        GitAiError::GitCliError { code, stderr, args } => {
            assert_eq!(code, Some(1));
            assert_eq!(stderr, "error message");
            assert_eq!(args, vec!["git".to_string(), "commit".to_string()]);
        }
        _ => panic!("Expected GitCliError"),
    }
}

#[test]
fn test_error_clone_utf8_error() {
    let invalid_utf8 = vec![0xFF];
    let utf8_err = std::str::from_utf8(&invalid_utf8).unwrap_err();
    let err = GitAiError::from(utf8_err);
    let cloned = err.clone();
    assert!(matches!(cloned, GitAiError::Utf8Error(_)));
}

#[test]
fn test_error_clone_from_utf8_error() {
    let invalid_utf8 = vec![0xFF];
    let from_utf8_err = String::from_utf8(invalid_utf8).unwrap_err();
    let err = GitAiError::from(from_utf8_err);
    let cloned = err.clone();
    assert!(matches!(cloned, GitAiError::FromUtf8Error(_)));
}

#[test]
fn test_error_clone_preset_error() {
    let err = GitAiError::PresetError("preset error".to_string());
    let cloned = err.clone();
    match cloned {
        GitAiError::PresetError(msg) => assert_eq!(msg, "preset error"),
        _ => panic!("Expected PresetError"),
    }
}

#[test]
fn test_error_clone_generic() {
    let err = GitAiError::Generic("generic".to_string());
    let cloned = err.clone();
    match cloned {
        GitAiError::Generic(msg) => assert_eq!(msg, "generic"),
        _ => panic!("Expected Generic"),
    }
}

#[test]
fn test_error_clone_json_converts_to_generic() {
    let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
    let err = GitAiError::from(json_err);
    let cloned = err.clone();
    assert!(matches!(cloned, GitAiError::Generic(_)));
    let display = format!("{}", cloned);
    assert!(display.contains("JSON error"));
}

#[test]
fn test_error_clone_sqlite_converts_to_generic() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("error.db");
    let conn = crate::model::repository::sqlite::open_with_memory_limits(&db_path).unwrap();
    let sql_err = conn.execute("BAD SQL", []).unwrap_err();
    let err = GitAiError::from(sql_err);
    let cloned = err.clone();
    assert!(matches!(cloned, GitAiError::Generic(_)));
    let display = format!("{}", cloned);
    assert!(display.contains("SQLite error"));
}

#[test]
fn test_error_clone_gix_converts_to_generic() {
    let err = GitAiError::GixError("gix error".to_string());
    let cloned = err.clone();
    assert!(matches!(cloned, GitAiError::Generic(_)));
    let display = format!("{}", cloned);
    assert!(display.contains("Gix error"));
}

#[test]
fn test_error_is_std_error() {
    let err = GitAiError::Generic("test".to_string());
    let _: &dyn std::error::Error = &err;
}

#[test]
fn test_error_debug_trait() {
    let err = GitAiError::Generic("debug test".to_string());
    let debug_str = format!("{:?}", err);
    assert!(debug_str.contains("Generic"));
    assert!(debug_str.contains("debug test"));
}

#[test]
fn test_error_persistence_display_delegates_to_inner() {
    use crate::model::repository::error::PersistenceError;
    let inner = PersistenceError::LockPoisoned { what: "notes-db" };
    let inner_text = inner.to_string();
    let err = GitAiError::Persistence(inner);
    // Must delegate — no "Generic error:" or other wrapper prefix
    assert_eq!(err.to_string(), inner_text);
}

#[test]
fn test_error_persistence_clone_is_non_lossy() {
    use crate::model::repository::error::PersistenceError;
    let inner = PersistenceError::Sqlite {
        db: "metrics",
        operation: "insert",
        code: Some(rusqlite::ffi::ErrorCode::DatabaseBusy),
        message: "busy".to_string(),
    };
    let err = GitAiError::Persistence(inner);
    let cloned = err.clone();
    assert!(
        matches!(cloned, GitAiError::Persistence(_)),
        "Persistence must not degrade to Generic on clone"
    );
}
