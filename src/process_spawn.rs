//! Layer-neutral process-spawn primitives shared by the git spawn layer,
//! CLI handlers, and daemon orchestration: Windows process-creation flags
//! and cached stdin-terminal detection. Lives at the crate root so the
//! clients layer never has to import from the interface (cli) layer.

use std::io::IsTerminal;

static IS_TERMINAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Whether stdin is an interactive terminal (cached for the process lifetime).
pub fn is_interactive_terminal() -> bool {
    *IS_TERMINAL.get_or_init(|| std::io::stdin().is_terminal())
}

/// Windows-specific flag to prevent console window creation
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x08000000;
/// Windows-specific flag to start a new process group
#[cfg(windows)]
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
/// Windows-specific flag to allow a child process to break away from the current job object
#[cfg(windows)]
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x01000000;

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[test]
    fn test_is_interactive_terminal() {
        // Just call it to ensure it doesn't panic
        let _ = is_interactive_terminal();
    }

    #[test]
    #[cfg(windows)]
    fn test_create_no_window_constant() {
        assert_eq!(CREATE_NO_WINDOW, 0x08000000);
    }

    #[test]
    #[cfg(windows)]
    fn test_create_new_process_group_constant() {
        assert_eq!(CREATE_NEW_PROCESS_GROUP, 0x00000200);
    }

    #[test]
    #[cfg(windows)]
    fn test_create_breakaway_from_job_constant() {
        assert_eq!(CREATE_BREAKAWAY_FROM_JOB, 0x01000000);
    }
}
