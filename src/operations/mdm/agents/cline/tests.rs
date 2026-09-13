use super::*;
use serial_test::serial;
use std::fs;
use std::sync::mpsc;
use std::time::Duration;
use tempfile::TempDir;

fn create_test_binary_path() -> PathBuf {
    PathBuf::from("/usr/local/bin/git-ai")
}

fn with_temp_home<F: FnOnce(&Path)>(f: F) {
    let temp_dir = TempDir::new().unwrap();
    let home = temp_dir.path().to_path_buf();

    let prev_home = std::env::var_os("HOME");
    let prev_userprofile = std::env::var_os("USERPROFILE");
    let prev_storage = std::env::var_os("GIT_AI_CLINE_STORAGE_PATH");

    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("USERPROFILE", &home);
    }

    f(&home);

    unsafe {
        match prev_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match prev_userprofile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        match prev_storage {
            Some(v) => std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", v),
            None => std::env::remove_var("GIT_AI_CLINE_STORAGE_PATH"),
        }
    }
}

#[test]
#[serial]
fn test_cline_check_not_installed() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };
        let result = ClineInstaller.check_hooks(&params).unwrap();
        assert!(!result.tool_installed);
        assert!(!result.hooks_installed);
        assert!(!result.hooks_up_to_date);
    });
}

#[test]
#[cfg(not(windows))]
#[serial]
fn test_cline_check_does_not_inspect_hooks_when_tool_is_absent() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        fs::create_dir_all(ClineInstaller::hooks_dir()).unwrap();
        fs::write(
            ClineInstaller::hook_path(PRE_HOOK_NAME),
            format!("#!/bin/sh\n{}\n", MANAGED_MARKER),
        )
        .unwrap();

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };
        let result = ClineInstaller.check_hooks(&params).unwrap();

        assert!(!result.tool_installed);
        assert!(!result.hooks_installed);
        assert!(!result.hooks_up_to_date);
    });
}

#[test]
#[cfg(not(windows))]
#[serial]
fn test_cline_check_propagates_hook_path_access_errors() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        fs::write(home.join("Documents"), "not a directory").unwrap();

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };
        let error = match ClineInstaller.check_hooks(&params) {
            Ok(_) => panic!("expected Cline hook access to fail"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("Cline hooks"),
            "unexpected error: {error}"
        );
    });
}

#[test]
fn test_cline_hook_check_timeout_is_bounded() {
    let (release_tx, release_rx) = mpsc::channel();
    let result: Result<(), GitAiError> =
        ClineInstaller::run_hook_check_with_timeout(Duration::from_millis(10), move || {
            release_rx.recv().unwrap();
            Ok(())
        });

    let error = match result {
        Ok(()) => panic!("expected Cline hook check to time out"),
        Err(error) => error,
    };
    assert!(
        error.to_string().contains("Timed out checking Cline hooks"),
        "unexpected error: {error}"
    );

    release_tx.send(()).unwrap();
}

#[test]
#[cfg(not(windows))]
#[serial]
fn test_cline_install_creates_hooks() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };

        let result = ClineInstaller.install_hooks(&params, false).unwrap();
        assert!(result.is_some(), "expected a diff");

        let pre_path = ClineInstaller::hook_path(PRE_HOOK_NAME);
        let post_path = ClineInstaller::hook_path(POST_HOOK_NAME);

        assert!(pre_path.exists());
        assert!(post_path.exists());

        let content = fs::read_to_string(&pre_path).unwrap();
        assert!(content.contains("git-ai-managed"));
        assert!(content.contains("checkpoint cline"));
        assert!(content.contains(r#"{"cancel":false}"#));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&pre_path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "hook should be executable");
        }

        let check = ClineInstaller.check_hooks(&params).unwrap();
        assert!(check.tool_installed);
        assert!(check.hooks_installed);
        assert!(check.hooks_up_to_date);
    });
}

#[test]
#[serial]
fn test_cline_install_is_idempotent() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };

        ClineInstaller.install_hooks(&params, false).unwrap();
        let second = ClineInstaller.install_hooks(&params, false).unwrap();
        assert!(second.is_none(), "second install should be a no-op");
    });
}

#[test]
#[serial]
fn test_cline_uninstall_removes_hooks() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };

        ClineInstaller.install_hooks(&params, false).unwrap();
        ClineInstaller.uninstall_hooks(&params, false).unwrap();

        assert!(!ClineInstaller::hook_path(PRE_HOOK_NAME).exists());
        assert!(!ClineInstaller::hook_path(POST_HOOK_NAME).exists());

        let check = ClineInstaller.check_hooks(&params).unwrap();
        assert!(check.tool_installed);
        assert!(!check.hooks_installed);
    });
}

#[test]
#[serial]
fn test_cline_uninstall_preserves_unmanaged_files() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        fs::create_dir_all(ClineInstaller::hooks_dir()).unwrap();
        let pre_path = ClineInstaller::hook_path(PRE_HOOK_NAME);
        fs::write(&pre_path, "#!/bin/sh\necho 'user hook'\n").unwrap();

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };

        let result = ClineInstaller.uninstall_hooks(&params, false).unwrap();
        assert!(result.is_none());
        assert!(pre_path.exists());
    });
}

#[test]
#[cfg(not(windows))]
#[serial]
fn test_cline_install_preserves_unmanaged_hook_without_partial_updates() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        fs::create_dir_all(ClineInstaller::hooks_dir()).unwrap();
        let pre_path = ClineInstaller::hook_path(PRE_HOOK_NAME);
        let post_path = ClineInstaller::hook_path(POST_HOOK_NAME);
        let managed_pre = format!("#!/bin/sh\n{}\necho 'stale'\n", MANAGED_MARKER);
        let unmanaged_post = "#!/bin/sh\necho 'user hook'\n";
        fs::write(&pre_path, &managed_pre).unwrap();
        fs::write(&post_path, unmanaged_post).unwrap();

        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };

        let error = ClineInstaller.install_hooks(&params, false).unwrap_err();
        assert!(error.to_string().contains("unmanaged Cline hook"));
        assert_eq!(fs::read_to_string(&pre_path).unwrap(), managed_pre);
        assert_eq!(fs::read_to_string(&post_path).unwrap(), unmanaged_post);
    });
}

#[test]
#[serial]
// Regression coverage for ENG-337.
fn test_cline_partial_managed_install_honors_platform_contract() {
    with_temp_home(|home| {
        let storage = home.join("cline-storage");
        fs::create_dir_all(&storage).unwrap();
        unsafe { std::env::set_var("GIT_AI_CLINE_STORAGE_PATH", &storage) };

        fs::create_dir_all(ClineInstaller::hooks_dir()).unwrap();
        let params = HookInstallerParams {
            binary_path: create_test_binary_path(),
        };
        let pre_path = ClineInstaller::hook_path(PRE_HOOK_NAME);
        let post_path = ClineInstaller::hook_path(POST_HOOK_NAME);
        let managed_pre = ClineInstaller::generate_hook_script(&params.binary_path);
        let unmanaged_post = "#!/bin/sh\necho 'user hook'\n";
        fs::write(&pre_path, &managed_pre).unwrap();
        fs::write(&post_path, unmanaged_post).unwrap();

        let check = ClineInstaller.check_hooks(&params).unwrap();
        assert!(check.tool_installed);
        #[cfg(not(windows))]
        {
            assert!(check.hooks_installed);
            assert!(!check.hooks_up_to_date);
            ClineInstaller.uninstall_hooks(&params, false).unwrap();
            assert!(!pre_path.exists());
            assert_eq!(fs::read_to_string(&post_path).unwrap(), unmanaged_post);
        }
        #[cfg(windows)]
        {
            assert!(!check.hooks_installed);
            assert!(!check.hooks_up_to_date);
            assert_eq!(ClineInstaller.install_hooks(&params, false).unwrap(), None);
            assert_eq!(fs::read_to_string(&pre_path).unwrap(), managed_pre);
            assert_eq!(fs::read_to_string(&post_path).unwrap(), unmanaged_post);
            assert_eq!(
                ClineInstaller.uninstall_hooks(&params, false).unwrap(),
                None
            );
            assert_eq!(fs::read_to_string(&pre_path).unwrap(), managed_pre);
            assert_eq!(fs::read_to_string(&post_path).unwrap(), unmanaged_post);
        }
    });
}
