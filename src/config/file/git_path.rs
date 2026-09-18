use std::path::Path;

#[cfg(not(windows))]
use crate::operations::mdm::paths::home_dir;

use super::{FileConfig, config_file_path, is_real_git_candidate};

pub(super) fn resolve_git_path(file_cfg: &Option<FileConfig>) -> String {
    // Nix wrappers supply their store Git without taking ownership of user config.
    let environment_path = std::env::var("GIT_AI_GIT_PATH").ok();
    let configured_path = file_cfg.as_ref().and_then(|cfg| cfg.git_path.as_deref());
    for path in [environment_path.as_deref(), configured_path]
        .into_iter()
        .flatten()
    {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let p = Path::new(trimmed);
            if is_real_git_candidate(p) {
                return trimmed.to_string();
            }
        }
    }

    // 2) Probe common locations across platforms.
    // All candidates are guarded by is_real_git_candidate so that a git-ai shim at any
    // of these locations can never be returned as the "real git" (fork bomb prevention).
    #[cfg(not(windows))]
    let local_bin_git = format!("{}/.local/bin/git", home_dir().display());

    #[cfg(windows)]
    let local_app_data_candidates: Vec<String> = std::env::var("LOCALAPPDATA")
        .ok()
        .map(|lad| {
            vec![
                format!(r"{}\Programs\Git\cmd\git.exe", lad),
                format!(r"{}\Programs\Git\bin\git.exe", lad),
            ]
        })
        .unwrap_or_default();

    let static_candidates: &[&str] = &[
        #[cfg(not(windows))]
        local_bin_git.as_str(),
        #[cfg(not(windows))]
        "/opt/homebrew/bin/git",
        #[cfg(not(windows))]
        "/usr/local/bin/git",
        #[cfg(not(windows))]
        "/usr/bin/git",
        #[cfg(not(windows))]
        "/bin/git",
        #[cfg(not(windows))]
        "/usr/local/sbin/git",
        #[cfg(not(windows))]
        "/usr/sbin/git",
        #[cfg(windows)]
        r"C:\Program Files\Git\cmd\git.exe",
        #[cfg(windows)]
        r"C:\Program Files\Git\bin\git.exe",
        #[cfg(windows)]
        r"C:\Program Files (x86)\Git\cmd\git.exe",
        #[cfg(windows)]
        r"C:\Program Files (x86)\Git\bin\git.exe",
    ];

    #[cfg(windows)]
    let all_candidates: Vec<&str> = {
        let mut v: Vec<&str> = static_candidates.to_vec();
        for c in &local_app_data_candidates {
            v.push(c.as_str());
        }
        v
    };

    #[cfg(windows)]
    let candidates: &[&str] = &all_candidates;
    #[cfg(not(windows))]
    let candidates: &[&str] = static_candidates;

    if let Some(found) = candidates
        .iter()
        .map(Path::new)
        .find(|p| is_real_git_candidate(p))
    {
        return found.to_string_lossy().to_string();
    }

    // 3) Windows-only: try `where.exe git.exe` as a PATH-based fallback
    #[cfg(windows)]
    {
        if let Ok(output) = std::process::Command::new("where.exe")
            .arg("git.exe")
            .output()
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                let p = Path::new(trimmed);
                if is_real_git_candidate(p) {
                    return trimmed.to_string();
                }
            }
        }
    }

    eprintln!(
        "Fatal: Could not locate a real 'git' binary.\n\
         Expected GIT_AI_GIT_PATH, a valid 'git_path' in {cfg_path}, or Git in standard locations.\n\
         Please install Git or update your config JSON.",
        cfg_path = config_file_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "~/.git-ai/config.json".to_string()),
    );
    std::process::exit(1);
}
