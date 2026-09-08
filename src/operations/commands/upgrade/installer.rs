#[cfg(windows)]
use super::{ENV_BACKGROUND_UPGRADE_WORKER, GIT_AI_RESTART_DAEMON_AFTER_INSTALL_ENV};
use super::{GIT_AI_DAEMON_UPGRADE_ENV, GIT_AI_RELEASE_ENV};
use std::fs;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
pub(super) type WindowsHandle = *mut std::ffi::c_void;
#[cfg(windows)]
pub(super) const TH32CS_SNAPPROCESS: u32 = 0x00000002;
#[cfg(windows)]
pub(super) const INVALID_HANDLE_VALUE: WindowsHandle = (-1isize) as WindowsHandle;
#[cfg(windows)]
pub(super) const WINDOWS_MAX_PATH: usize = 260;

#[cfg(windows)]
#[repr(C)]
pub(super) struct ProcessEntry32W {
    pub(super) dw_size: u32,
    pub(super) cnt_usage: u32,
    pub(super) th32_process_id: u32,
    pub(super) th32_default_heap_id: usize,
    pub(super) th32_module_id: u32,
    pub(super) cnt_threads: u32,
    pub(super) th32_parent_process_id: u32,
    pub(super) pc_pri_class_base: i32,
    pub(super) dw_flags: u32,
    pub(super) sz_exe_file: [u16; WINDOWS_MAX_PATH],
}

#[cfg(windows)]
unsafe extern "system" {
    pub(super) fn CreateToolhelp32Snapshot(flags: u32, process_id: u32) -> WindowsHandle;
    pub(super) fn Process32FirstW(snapshot: WindowsHandle, entry: *mut ProcessEntry32W) -> i32;
    pub(super) fn Process32NextW(snapshot: WindowsHandle, entry: *mut ProcessEntry32W) -> i32;
    pub(super) fn CloseHandle(handle: WindowsHandle) -> i32;
}

#[cfg(windows)]
pub(super) fn exit_if_invoked_via_git_extension() {
    if should_block_git_extension_upgrade(
        parent_process_name().as_deref(),
        std::env::var(ENV_BACKGROUND_UPGRADE_WORKER).as_deref() == Ok("1"),
    ) {
        eprintln!(
            "error: `git ai upgrade` is not supported on Windows. Run `git-ai upgrade` instead."
        );
        std::process::exit(1);
    }
}

#[cfg(windows)]
pub(super) fn should_block_git_extension_upgrade(
    parent_process_name: Option<&str>,
    is_background_worker: bool,
) -> bool {
    !is_background_worker && parent_process_name.is_some_and(is_git_process_name)
}

#[cfg(windows)]
pub(super) fn is_git_process_name(name: &str) -> bool {
    std::path::Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| {
            file_name.eq_ignore_ascii_case("git") || file_name.eq_ignore_ascii_case("git.exe")
        })
}

#[cfg(windows)]
pub(super) fn parent_process_name() -> Option<String> {
    struct SnapshotGuard(WindowsHandle);

    impl Drop for SnapshotGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let _snapshot_guard = SnapshotGuard(snapshot);

    let current_pid = std::process::id();
    let parent_pid = find_parent_pid(snapshot, current_pid)?;
    process_name_for_pid(snapshot, parent_pid)
}

#[cfg(windows)]
pub(super) fn find_parent_pid(snapshot: WindowsHandle, current_pid: u32) -> Option<u32> {
    let mut entry = windows_process_entry_template();
    if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
        return None;
    }

    loop {
        if entry.th32_process_id == current_pid {
            return Some(entry.th32_parent_process_id);
        }
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            return None;
        }
    }
}

#[cfg(windows)]
pub(super) fn process_name_for_pid(snapshot: WindowsHandle, pid: u32) -> Option<String> {
    let mut entry = windows_process_entry_template();
    if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
        return None;
    }

    loop {
        if entry.th32_process_id == pid {
            let len = entry
                .sz_exe_file
                .iter()
                .position(|&ch| ch == 0)
                .unwrap_or(entry.sz_exe_file.len());
            return Some(String::from_utf16_lossy(&entry.sz_exe_file[..len]));
        }
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            return None;
        }
    }
}

#[cfg(windows)]
pub(super) fn windows_process_entry_template() -> ProcessEntry32W {
    ProcessEntry32W {
        dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
        cnt_usage: 0,
        th32_process_id: 0,
        th32_default_heap_id: 0,
        th32_module_id: 0,
        cnt_threads: 0,
        th32_parent_process_id: 0,
        pc_pri_class_base: 0,
        dw_flags: 0,
        sz_exe_file: [0; WINDOWS_MAX_PATH],
    }
}

pub(super) fn run_install_script(
    script_content: &str,
    tag: &str,
    silent: bool,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        if let Ok(daemon_config) =
            crate::operations::daemon::DaemonConfig::from_env_or_default_paths()
        {
            // Best effort: stop the daemon before we hand off to the detached installer.
            // The install script also has a fallback kill path so old released binaries
            // can still recover, but stopping here makes upgrades complete sooner.
            let _ = crate::operations::commands::daemon::stop_daemon(
                &daemon_config,
                Duration::from_secs(10),
            );
        }

        // On Windows, we need to run the installer detached because the current git-ai
        // binary and shims are in use and need to be replaced. The installer will wait
        // for the files to be released before proceeding.
        let pid = std::process::id();
        let log_dir = dirs::home_dir()
            .ok_or_else(|| "Could not determine home directory".to_string())?
            .join(".git-ai")
            .join("upgrade-logs");

        // Ensure the log directory exists
        fs::create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log directory: {}", e))?;

        let log_file = log_dir.join(format!("upgrade-{}.log", pid));
        let log_path_str = log_file.to_string_lossy().to_string();

        // Write the install script to a temp file
        let script_path = log_dir.join(format!("install-{}.ps1", pid));
        fs::write(&script_path, script_content)
            .map_err(|e| format!("Failed to write install script: {}", e))?;
        let script_path_str = script_path.to_string_lossy().to_string();

        // Create log file with initial message
        fs::write(&log_file, format!("Starting upgrade at PID {}\n", pid))
            .map_err(|e| format!("Failed to create log file: {}", e))?;

        // PowerShell wrapper that executes the script file with logging
        let ps_wrapper = format!(
            "$logFile = '{}'; \
             Start-Transcript -Path $logFile -Append -Force | Out-Null; \
             Write-Host 'Running verified install script...'; \
             try {{ \
                  $ErrorActionPreference = 'Continue'; \
                  & '{}'; \
                  Write-Host 'Install script completed'; \
              }} catch {{ \
                  Write-Host \"Error: $_\"; \
                  Write-Host \"Stack trace: $($_.ScriptStackTrace)\"; \
              }} finally {{ \
                  if ($env:{} -eq '1') {{ \
                      $daemonExe = Join-Path $HOME '.git-ai\\bin\\git-ai.exe'; \
                      if (Test-Path $daemonExe) {{ try {{ & $daemonExe bg start *> $null }} catch {{ }} }} \
                  }}; \
                  Stop-Transcript | Out-Null; \
                  Remove-Item -Path '{}' -Force -ErrorAction SilentlyContinue; \
              }}",
            log_path_str, script_path_str, GIT_AI_RESTART_DAEMON_AFTER_INSTALL_ENV, script_path_str
        );

        let spawn_powershell = |exe: &str| -> std::io::Result<std::process::Child> {
            let mut cmd = Command::new(exe);
            cmd.arg("-NoProfile")
                .arg("-ExecutionPolicy")
                .arg("Bypass")
                .arg("-Command")
                .arg(&ps_wrapper)
                .env(GIT_AI_RELEASE_ENV, tag);

            // Hide the spawned console to prevent any host/UI bleed-through
            cmd.creation_flags(crate::process_spawn::CREATE_NO_WINDOW);

            if silent {
                cmd.env(GIT_AI_RESTART_DAEMON_AFTER_INSTALL_ENV, "1");
                cmd.env(GIT_AI_DAEMON_UPGRADE_ENV, "1");
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }

            cmd.spawn()
        };

        let spawn_result = spawn_powershell("pwsh").or_else(|_| spawn_powershell("powershell"));

        match spawn_result {
            Ok(_) => {
                if !silent {
                    println!(
                        "\x1b[1;33mNote: The installation is running in the background on Windows.\x1b[0m"
                    );
                    println!(
                        "This allows the current git-ai process to exit and release file locks."
                    );
                    println!("Check the log file for progress: {}", log_path_str);
                    println!(
                        "The installer will stop lingering git-ai background processes if needed, but active git commands can still delay completion."
                    );
                }
                Ok(())
            }
            Err(e) => Err(format!("Failed to run installation script: {}", e)),
        }
    }

    #[cfg(not(windows))]
    {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        // Write script to ~/.git-ai/tmp/ to avoid /tmp noexec or permission issues.
        // Fall back to the system temp dir if the home-based path is unavailable.
        let temp_dir = crate::config::git_ai_dir_path()
            .map(|p| p.join("tmp"))
            .unwrap_or_else(std::env::temp_dir);
        fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp directory: {}", e))?;
        let script_path = temp_dir.join(format!("git-ai-install-{}.sh", std::process::id()));

        // Write and make executable
        let mut file = fs::File::create(&script_path)
            .map_err(|e| format!("Failed to create temp script file: {}", e))?;
        file.write_all(script_content.as_bytes())
            .map_err(|e| format!("Failed to write install script: {}", e))?;
        drop(file);

        fs::set_permissions(&script_path, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to make script executable: {}", e))?;

        let script_path_str = script_path.to_string_lossy().to_string();

        let mut cmd = Command::new("bash");
        cmd.arg(&script_path_str).env(GIT_AI_RELEASE_ENV, tag);

        if silent {
            cmd.env(GIT_AI_DAEMON_UPGRADE_ENV, "1");
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }

        let result = match cmd.status() {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "Installation script failed with exit code: {:?}",
                        status.code()
                    ))
                }
            }
            Err(e) => Err(format!("Failed to run installation script: {}", e)),
        };

        // Clean up temp script
        let _ = fs::remove_file(&script_path);

        result
    }
}
