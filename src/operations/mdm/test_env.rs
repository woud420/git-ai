//! Shared test-only environment fixtures for the mdm agent installers.
//!
//! Every agent installer's test module needs to sandbox `HOME`/`USERPROFILE`
//! (and sometimes `PATH`) so detection logic never touches the real user
//! environment. These helpers were duplicated verbatim across ~10 agent test
//! modules; this is the single canonical copy. Agent modules that also need
//! to sandbox an agent-specific env var (e.g. `CODEX_HOME`, `GEMINI_CLI_HOME`,
//! `GIT_AI_CLINE_STORAGE_PATH`) keep their own local variant instead of using
//! this one, since the extra save/restore isn't shared behavior.

use std::path::Path;
use tempfile::TempDir;

/// Temporarily override `HOME` (and `USERPROFILE` on Windows) to a fresh temp
/// directory for the duration of the closure. Must only be called from
/// `#[serial]` tests to avoid racing with other tests that read `HOME`.
pub(crate) fn with_temp_home<F: FnOnce(&Path)>(f: F) {
    let temp_dir = TempDir::new().unwrap();
    let home = temp_dir.path().to_path_buf();

    let prev_home = std::env::var_os("HOME");
    let prev_userprofile = std::env::var_os("USERPROFILE");

    // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
    }

    f(&home);

    // SAFETY: tests are serialized via #[serial], so restoring process env is safe.
    unsafe {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
    }
}

/// Temporarily point `PATH` at a directory containing a single fake,
/// executable `binary_name` script, for the duration of the closure. Must
/// only be called from `#[serial]` tests to avoid racing with other tests
/// that read `PATH`.
pub(crate) fn with_fake_binary_on_path<F: FnOnce(&Path)>(binary_name: &str, f: F) {
    let temp_dir = TempDir::new().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let fake_bin = bin_dir.join(binary_name);
    std::fs::write(&fake_bin, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake_bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let prev_path = std::env::var_os("PATH");
    let new_path = match &prev_path {
        Some(p) => {
            let mut paths = vec![bin_dir.clone()];
            paths.extend(std::env::split_paths(p));
            std::env::join_paths(paths).unwrap()
        }
        None => bin_dir.clone().into(),
    };

    // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
    unsafe {
        std::env::set_var("PATH", &new_path);
    }

    f(temp_dir.path());

    // SAFETY: tests are serialized via #[serial], so restoring process env is safe.
    unsafe {
        match prev_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
}

/// Temporarily point `PATH` at an empty temp directory (i.e. no binaries are
/// resolvable) for the duration of the closure. Must only be called from
/// `#[serial]` tests to avoid racing with other tests that read `PATH`.
pub(crate) fn with_empty_path<F: FnOnce()>(f: F) {
    let temp_dir = TempDir::new().unwrap();
    let prev_path = std::env::var_os("PATH");

    // SAFETY: tests are serialized via #[serial], so mutating process env is safe.
    unsafe {
        std::env::set_var("PATH", temp_dir.path());
    }

    f();

    // SAFETY: tests are serialized via #[serial], so restoring process env is safe.
    unsafe {
        match prev_path {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
}
