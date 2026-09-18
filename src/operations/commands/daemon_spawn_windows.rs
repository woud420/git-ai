use crate::process_spawn::{CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{
    ERROR_INVALID_HANDLE, GetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE,
    SetHandleInformation,
};

static DETACHED_SPAWN_LOCK: Mutex<()> = Mutex::new(());

pub(super) fn spawn(child: &mut Command) -> Result<(), String> {
    // Restoring flags while another detached spawn is still running would
    // re-enable inheritance too early. Ordinary Rust Command spawns duplicate
    // their explicit stdio handles and do not depend on these original flags.
    let _spawn_lock = DETACHED_SPAWN_LOCK
        .lock()
        .map_err(|_| "detached daemon spawn lock poisoned".to_string())?;
    let _stdio = StdioInheritanceGuard::new([
        std::io::stdin().as_raw_handle(),
        std::io::stdout().as_raw_handle(),
        std::io::stderr().as_raw_handle(),
    ])
    .map_err(|error| format!("failed to isolate detached daemon standard handles: {error}"))?;
    let preferred_flags = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB;
    child.creation_flags(preferred_flags);
    match child.spawn() {
        Ok(_) => Ok(()),
        Err(preferred_err) => {
            tracing::debug!(
                "detached daemon spawn with CREATE_BREAKAWAY_FROM_JOB failed, retrying without it: {}",
                preferred_err
            );
            child.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
            child.spawn().map(|_| ()).map_err(|fallback_err| format!(
                "failed to spawn detached daemon with flags {preferred_flags:#x}: {preferred_err}; retry without CREATE_BREAKAWAY_FROM_JOB also failed: {fallback_err}"
            ))
        }
    }
}

struct StdioInheritanceGuard {
    changed: Vec<(HANDLE, u32)>,
}

impl StdioInheritanceGuard {
    fn new(handles: [HANDLE; 3]) -> std::io::Result<Self> {
        let mut guard = Self {
            changed: Vec::with_capacity(3),
        };
        for handle in handles {
            if handle.is_null()
                || handle == INVALID_HANDLE_VALUE
                || guard.changed.iter().any(|(seen, _)| *seen == handle)
            {
                continue;
            }
            let mut flags = 0;
            // Handles are borrowed from live stdio owners; these APIs inspect
            // or change flags without transferring ownership or closing them.
            if unsafe { GetHandleInformation(handle, &mut flags) } == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_INVALID_HANDLE as i32) {
                    continue;
                }
                return Err(error);
            }
            if flags & HANDLE_FLAG_INHERIT != 0 {
                if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
                    return Err(std::io::Error::last_os_error());
                }
                guard.changed.push((handle, flags));
            }
        }
        Ok(guard)
    }
}

impl Drop for StdioInheritanceGuard {
    fn drop(&mut self) {
        for &(handle, flags) in self.changed.iter().rev() {
            // The owners outlive this guard. Restore only inheritance so any
            // unrelated handle flags are preserved, including on spawn failure.
            if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, flags) } == 0 {
                tracing::warn!(error = %std::io::Error::last_os_error(), "failed to restore standard-handle inheritance");
            }
        }
    }
}

#[cfg(test)]
#[path = "daemon_spawn_windows_tests.rs"]
mod tests;
