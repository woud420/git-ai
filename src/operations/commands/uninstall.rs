//! `git-ai uninstall` — remove git-ai integrations from this machine.
//!
//! Steps (each best-effort; a failure is reported and the remaining steps
//! still run):
//! 1. Remove agent/IDE hooks (reuses the `uninstall-hooks` machinery).
//! 2. Revert the global git trace2 config if it points at the git-ai daemon.
//! 3. Shut the daemon down and remove its sockets/locks.
//! 4. Remove installed binaries and shims (`~/.git-ai/bin`,
//!    `~/.local/bin/git-ai`).
//! 5. Remove installer-added PATH lines from shell rc files.
//! 6. With `--purge`, remove the whole `~/.git-ai` data directory (config,
//!    local databases, logs).
//!
//! Repo-local `.git/ai/` directories are never touched: git-ai does not track
//! which repositories exist, so they are the user's to remove.

use crate::config::Config;
use crate::error::GitAiError;
use crate::operations::commands::install_hooks::{
    TRACE2_EVENT_NESTING_KEY, TRACE2_EVENT_TARGET_KEY, remove_global_git_config_section,
};
use std::io::IsTerminal;
use std::path::Path;

const RC_MARKER_COMMENT: &str = "# Added by git-ai installer";

pub fn run_uninstall_all(args: &[String]) -> Result<(), GitAiError> {
    let mut purge = false;
    let mut assume_yes = false;
    for arg in args {
        match arg.as_str() {
            "--purge" => purge = true,
            "--yes" | "-y" => assume_yes = true,
            "--help" | "-h" => {
                print_help();
                return Ok(());
            }
            other => {
                return Err(GitAiError::Generic(format!(
                    "unknown option '{}'; run `git-ai uninstall --help` for usage",
                    other
                )));
            }
        }
    }

    if !assume_yes && !confirm(purge) {
        println!("Aborted.");
        return Ok(());
    }

    let mut report: Vec<String> = Vec::new();

    // 1. Agent/IDE hooks.
    match crate::operations::commands::install_hooks::run_uninstall(&[]) {
        Ok(_) => report.push("agent hooks: removed".to_string()),
        Err(e) => report.push(format!("agent hooks: FAILED ({})", e)),
    }

    // 2. Global git trace2 config — only when it points at our daemon.
    report.push(revert_trace2_config());

    // 3. Daemon shutdown + socket/lock cleanup. Skipped inside test harnesses
    //    (mirrors ensure_daemon), where per-test daemons manage their own state.
    if std::env::var_os("GIT_AI_TEST_DB_PATH").is_none()
        && std::env::var_os("GITAI_TEST_DB_PATH").is_none()
    {
        crate::operations::commands::daemon::handle_daemon(&["shutdown".to_string()]);
        report.push("daemon: shutdown requested".to_string());
    }
    if let Some(home) = dirs::home_dir() {
        let daemon_dir = home.join(".git-ai").join("internal").join("daemon");
        if daemon_dir.exists() {
            match std::fs::remove_dir_all(&daemon_dir) {
                Ok(()) => report.push("daemon sockets/locks: removed".to_string()),
                Err(e) => report.push(format!("daemon sockets/locks: FAILED ({})", e)),
            }
        }
    }

    // 4. Binaries and shims.
    report.extend(remove_binaries());

    // 5. Shell rc PATH lines.
    report.extend(clean_shell_rc_files());

    // 6. Data.
    if purge {
        if let Some(home) = dirs::home_dir() {
            let data_dir = home.join(".git-ai");
            if data_dir.exists() {
                match std::fs::remove_dir_all(&data_dir) {
                    Ok(()) => report.push("~/.git-ai (config + data): removed".to_string()),
                    Err(e) => report.push(format!("~/.git-ai: FAILED ({})", e)),
                }
            }
        }
    } else {
        report.push(
            "~/.git-ai (config + local databases): kept — remove with `git-ai uninstall --purge`"
                .to_string(),
        );
    }

    println!("\ngit-ai uninstall summary:");
    for line in &report {
        println!("  - {}", line);
    }
    println!(
        "\nNote: repo-local .git/ai directories are not tracked and were left in place.\n\
         If you installed via Nix or Home Manager, remove git-ai from your Nix configuration as well."
    );
    Ok(())
}

fn confirm(purge: bool) -> bool {
    if !std::io::stdin().is_terminal() {
        eprintln!("Refusing to uninstall without a terminal; pass --yes to proceed.");
        return false;
    }
    if purge {
        println!(
            "This removes all git-ai integrations AND deletes ~/.git-ai (config, local attribution databases)."
        );
    } else {
        println!("This removes all git-ai integrations (hooks, git config, daemon, binaries).");
    }
    print!("Continue? [y/N] ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}

/// Remove our trace2 configuration from the global git config, but only when
/// the event target actually points at the git-ai daemon (never clobber a
/// user's own trace2 setup).
fn revert_trace2_config() -> String {
    let config = Config::fresh();
    let git_cmd = config.git_cmd().to_string();

    let current_target = std::process::Command::new(&git_cmd)
        .args(["config", "--global", "--get", TRACE2_EVENT_TARGET_KEY])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    if current_target.is_empty() {
        return "git trace2 config: not set".to_string();
    }
    if !current_target.contains(".git-ai") {
        return format!(
            "git trace2 config: left untouched ({} does not point at git-ai)",
            TRACE2_EVENT_TARGET_KEY
        );
    }
    match remove_global_git_config_section(&git_cmd, "trace2") {
        Ok(()) => format!(
            "git trace2 config: removed ({}, {})",
            TRACE2_EVENT_TARGET_KEY, TRACE2_EVENT_NESTING_KEY
        ),
        Err(e) => format!("git trace2 config: FAILED ({})", e),
    }
}

fn remove_binaries() -> Vec<String> {
    let mut report = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return report;
    };

    // ~/.local/bin/git-ai symlink (only when it points into ~/.git-ai).
    let local_bin_link = home.join(".local").join("bin").join("git-ai");
    if let Ok(target) = std::fs::read_link(&local_bin_link)
        && target.to_string_lossy().contains(".git-ai")
    {
        match std::fs::remove_file(&local_bin_link) {
            Ok(()) => report.push("~/.local/bin/git-ai symlink: removed".to_string()),
            Err(e) => report.push(format!("~/.local/bin/git-ai symlink: FAILED ({})", e)),
        }
    }

    // ~/.git-ai/bin — the installed binary and git shim. On Unix the currently
    // running binary can be unlinked; on Windows deletion of a running exe
    // fails, so leave instructions instead.
    let bin_dir = home.join(".git-ai").join("bin");
    if bin_dir.exists() {
        #[cfg(windows)]
        {
            report.push(format!(
                "{}: remove manually after this process exits (Windows cannot delete a running executable)",
                bin_dir.display()
            ));
        }
        #[cfg(not(windows))]
        match std::fs::remove_dir_all(&bin_dir) {
            Ok(()) => report.push("~/.git-ai/bin (binary + git shim): removed".to_string()),
            Err(e) => report.push(format!("~/.git-ai/bin: FAILED ({})", e)),
        }
    }
    report
}

/// Strip installer-added PATH lines from the standard shell rc files: the
/// `# Added by git-ai installer …` marker comment and any line referencing
/// `/.git-ai/bin`.
fn clean_shell_rc_files() -> Vec<String> {
    let mut report = Vec::new();
    let Some(home) = dirs::home_dir() else {
        return report;
    };
    let rc_files = [
        home.join(".bashrc"),
        home.join(".bash_profile"),
        home.join(".zshrc"),
        home.join(".config").join("fish").join("config.fish"),
    ];
    for rc in rc_files {
        match clean_rc_file(&rc) {
            Ok(true) => report.push(format!("{}: git-ai PATH lines removed", display_home(&rc))),
            Ok(false) => {}
            Err(e) => report.push(format!("{}: FAILED ({})", display_home(&rc), e)),
        }
    }
    report
}

fn clean_rc_file(path: &Path) -> Result<bool, std::io::Error> {
    if !path.is_file() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(path)?;
    let cleaned: Vec<&str> = content
        .lines()
        .filter(|line| !line.starts_with(RC_MARKER_COMMENT) && !line.contains("/.git-ai/bin"))
        .collect();
    let mut cleaned = cleaned.join("\n");
    if content.ends_with('\n') && !cleaned.ends_with('\n') {
        cleaned.push('\n');
    }
    if cleaned == content {
        return Ok(false);
    }
    std::fs::write(path, cleaned)?;
    Ok(true)
}

fn display_home(path: &Path) -> String {
    match dirs::home_dir() {
        Some(home) => match path.strip_prefix(&home) {
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => path.display().to_string(),
        },
        None => path.display().to_string(),
    }
}

fn print_help() {
    println!("git-ai uninstall - Remove git-ai integrations from this machine");
    println!();
    println!("Usage:");
    println!("  git-ai uninstall [--yes] [--purge]");
    println!();
    println!("Options:");
    println!("  --yes, -y   Skip the confirmation prompt");
    println!("  --purge     Also delete ~/.git-ai (config and local attribution databases)");
    println!();
    println!("Removes agent hooks, the global git trace2 config, the daemon, installed");
    println!("binaries/shims, and installer-added PATH lines. Without --purge, your");
    println!("configuration and local attribution data are kept.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_rc_file_strips_installer_lines() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        std::fs::write(
            &rc,
            "export FOO=1\n# Added by git-ai installer on Sun Jul 20\nexport PATH=\"/Users/x/.git-ai/bin:$PATH\"\nalias ll='ls -l'\n",
        )
        .unwrap();

        assert!(clean_rc_file(&rc).unwrap());
        let cleaned = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(cleaned, "export FOO=1\nalias ll='ls -l'\n");

        // Idempotent: a second pass changes nothing.
        assert!(!clean_rc_file(&rc).unwrap());
    }

    #[test]
    fn clean_rc_file_leaves_unrelated_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".bashrc");
        let content = "export FOO=1\nexport PATH=\"$HOME/bin:$PATH\"\n";
        std::fs::write(&rc, content).unwrap();

        assert!(!clean_rc_file(&rc).unwrap());
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), content);
    }

    #[test]
    fn clean_rc_file_handles_fish_add_path() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join("config.fish");
        std::fs::write(
            &rc,
            "# Added by git-ai installer on Sun Jul 20\nfish_add_path -g \"/Users/x/.git-ai/bin\"\nset -x EDITOR vim\n",
        )
        .unwrap();

        assert!(clean_rc_file(&rc).unwrap());
        assert_eq!(std::fs::read_to_string(&rc).unwrap(), "set -x EDITOR vim\n");
    }

    #[test]
    fn clean_rc_file_missing_file_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!clean_rc_file(&dir.path().join("nope")).unwrap());
    }
}
