//! Runtime environment detection for CLI startup checks: interactive
//! terminal, background AI agent, and superuser (root/Administrator) guards.

/// Returns true if the process is running inside a background AI agent environment.
pub fn is_in_background_agent() -> bool {
    !matches!(
        crate::operations::authorship::background_agent::detect(),
        crate::operations::authorship::background_agent::BackgroundAgent::None
    )
}

/// Returns true if the current process is running with elevated privileges
/// (root on Unix, Administrator on Windows).
#[cfg(unix)]
pub fn is_running_as_superuser() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
pub fn is_running_as_superuser() -> bool {
    use std::ffi::c_void;
    use std::mem;

    type Handle = *mut c_void;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn OpenProcessToken(
            process_handle: Handle,
            desired_access: u32,
            token_handle: *mut Handle,
        ) -> i32;
        fn GetTokenInformation(
            token_handle: Handle,
            token_information_class: u32,
            token_information: *mut u8,
            token_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
    }

    const TOKEN_QUERY: u32 = 0x0008;
    // TokenElevationType (class 18) returns 1/2/3:
    //   1 = Default (no split token — UAC disabled or built-in Admin)
    //   2 = Full (elevated half of split token — "Run as Administrator")
    //   3 = Limited (non-elevated half of split token — normal terminal)
    // Only type 2 is the dangerous case: files will be admin-owned but normal
    // processes won't be, causing permission mismatches.
    const TOKEN_ELEVATION_TYPE_CLASS: u32 = 18;
    const TOKEN_ELEVATION_TYPE_FULL: u32 = 2;

    unsafe {
        let mut token: Handle = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elev_type: u32 = 0;
        let mut size: u32 = 0;
        let result = GetTokenInformation(
            token,
            TOKEN_ELEVATION_TYPE_CLASS,
            &mut elev_type as *mut _ as *mut u8,
            mem::size_of::<u32>() as u32,
            &mut size,
        );
        CloseHandle(token);

        result != 0 && elev_type == TOKEN_ELEVATION_TYPE_FULL
    }
}

/// Returns true if the environment indicates a CI system or automated agent
/// sandbox where running as superuser is expected and acceptable.
pub fn is_superuser_expected_environment() -> bool {
    if std::env::var_os("CI").is_some() {
        return true;
    }
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        return true;
    }
    if std::env::var_os("GITLAB_CI").is_some() {
        return true;
    }
    if std::env::var_os("JENKINS_URL").is_some() {
        return true;
    }
    if std::env::var_os("BUILDKITE").is_some() {
        return true;
    }
    if std::env::var_os("CIRCLECI").is_some() {
        return true;
    }
    if std::env::var_os("CODEBUILD_BUILD_ID").is_some() {
        return true;
    }
    if std::env::var_os("AGENT_OS").is_some() {
        return true;
    }
    if std::env::var_os("KUBERNETES_SERVICE_HOST").is_some() {
        return true;
    }
    if is_inside_container() {
        return true;
    }
    if std::env::var_os("GIT_AI_DAEMON_UPGRADE").is_some() {
        return true;
    }
    false
}

fn is_inside_container() -> bool {
    // `container` env var is set by podman, systemd-nspawn, and other runtimes
    if std::env::var_os("container").is_some() {
        return true;
    }
    // Docker creates /.dockerenv in every container
    #[cfg(unix)]
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }
    false
}

/// Returns true if the user has explicitly opted in to running as superuser
/// via the `GIT_AI_ALLOW_SUPERUSER` env var or `allow_superuser` config flag.
pub fn superuser_is_allowed() -> bool {
    std::env::var("GIT_AI_ALLOW_SUPERUSER")
        .is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        || crate::config::Config::get().allow_superuser()
}

pub enum SuperuserCheckResult {
    Allowed,
    AllowedWithWarning,
    WarnFutureBlock,
}

/// Checks whether the current process is running with elevated privileges.
/// Returns `Allowed` if not superuser or in CI/agent environments.
/// Returns `AllowedWithWarning` if user explicitly opted in.
/// Returns `WarnFutureBlock` if running as superuser without opt-in (warn-only
/// for now; a future version will block).
pub fn check_superuser_guard() -> SuperuserCheckResult {
    if !is_running_as_superuser() {
        return SuperuserCheckResult::Allowed;
    }
    if is_superuser_expected_environment() {
        return SuperuserCheckResult::Allowed;
    }
    if superuser_is_allowed() {
        return SuperuserCheckResult::AllowedWithWarning;
    }
    SuperuserCheckResult::WarnFutureBlock
}

pub fn print_superuser_warning() {
    eprintln!(
        "[git-ai] warning: running as superuser (root/Administrator) is not recommended.\n\
         \n\
         Running with elevated privileges creates files owned by root that become\n\
         inaccessible to your normal user account, causing persistent daemon lock\n\
         failures. A future version may refuse to run in this configuration.\n\
         \n\
         To suppress this warning, either:\n\
         - Run git-ai as your normal user (recommended), or\n\
         - Set GIT_AI_ALLOW_SUPERUSER=1 or add \"allow_superuser\": true to ~/.git-ai/config.json\n\
         \n\
         This warning is automatically suppressed in CI environments."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn test_is_superuser_expected_environment_ci() {
        let had_ci = std::env::var_os("CI");
        unsafe { std::env::set_var("CI", "true") };
        assert!(is_superuser_expected_environment());
        match had_ci {
            Some(v) => unsafe { std::env::set_var("CI", v) },
            None => unsafe { std::env::remove_var("CI") },
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_superuser_is_allowed_env_var() {
        let had_var = std::env::var_os("GIT_AI_ALLOW_SUPERUSER");
        unsafe { std::env::set_var("GIT_AI_ALLOW_SUPERUSER", "1") };
        assert!(superuser_is_allowed());
        unsafe { std::env::set_var("GIT_AI_ALLOW_SUPERUSER", "true") };
        assert!(superuser_is_allowed());
        unsafe { std::env::set_var("GIT_AI_ALLOW_SUPERUSER", "TRUE") };
        assert!(superuser_is_allowed());
        unsafe { std::env::set_var("GIT_AI_ALLOW_SUPERUSER", "0") };
        assert!(!superuser_is_allowed());
        unsafe { std::env::remove_var("GIT_AI_ALLOW_SUPERUSER") };
        assert!(!superuser_is_allowed());
        match had_var {
            Some(v) => unsafe { std::env::set_var("GIT_AI_ALLOW_SUPERUSER", v) },
            None => unsafe { std::env::remove_var("GIT_AI_ALLOW_SUPERUSER") },
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_is_running_as_superuser_reports_correctly() {
        let euid = unsafe { libc::geteuid() };
        assert_eq!(is_running_as_superuser(), euid == 0);
    }
}
