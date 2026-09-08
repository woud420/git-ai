use super::FileConfig;
use super::storage::config_file_path;
#[cfg(unix)]
use crate::operations::mdm::paths::home_dir;
#[cfg(unix)]
use std::fs;
use std::path::Path;

pub(super) fn resolve_git_path(file_cfg: &Option<FileConfig>) -> String {
    // 1) From config file
    if let Some(cfg) = file_cfg
        && let Some(path) = cfg.git_path.as_ref()
    {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            let p = Path::new(trimmed);
            if is_executable(p) && !path_is_git_ai_binary(p) {
                return trimmed.to_string();
            }
        }
    }

    // 2) Probe common locations across platforms.
    // All candidates are guarded by path_is_git_ai_binary so that a git-ai shim at any
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
        .find(|p| is_executable(p) && !path_is_git_ai_binary(p))
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
                if is_executable(p) && !path_is_git_ai_binary(p) {
                    return trimmed.to_string();
                }
            }
        }
    }

    eprintln!(
        "Fatal: Could not locate a real 'git' binary.\n\
         Expected a valid 'git_path' in {cfg_path} or in standard locations.\n\
         Please install Git or update your config JSON.",
        cfg_path = config_file_path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "~/.git-ai/config.json".to_string()),
    );
    std::process::exit(1);
}

pub(super) fn is_executable(path: &Path) -> bool {
    if !path.exists() || !path.is_file() {
        return false;
    }
    // Basic check: existence is sufficient for our purposes; OS will enforce exec perms.
    // On Unix we could check permissions, but many filesystems differ. Keep it simple.
    true
}

/// Check whether two paths refer to the same underlying file.
/// On Unix this compares (dev, ino); on other platforms it falls back to
/// comparing canonicalized paths.
#[cfg(not(windows))]
pub(super) fn same_file(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (fs::metadata(a), fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
    }
    #[cfg(not(unix))]
    {
        if let (Ok(ca), Ok(cb)) = (a.canonicalize(), b.canonicalize()) {
            return ca == cb;
        }
    }
    false
}

/// Detect if a path is actually the git-ai binary (or a symlink to it).
/// This prevents `git_cmd()` from returning the git-ai shim, which would
/// cause infinite recursion: handle_git() → proxy_to_git() → shim → handle_git() → ...
pub(crate) fn path_is_git_ai_binary(path: &Path) -> bool {
    // Check canonical path — if the path resolves to a binary whose name
    // is git-ai (or a variant), it is the git-ai binary regardless of what
    // the original path looks like (catches symlinks like `git → git-ai`).
    if let Ok(canonical) = path.canonicalize()
        && let Some(name) = canonical.file_name().and_then(|n| n.to_str())
    {
        let stem = name.strip_suffix(".exe").unwrap_or(name);
        if stem == "git-ai" || stem.starts_with("git-ai-") || stem.starts_with("git_ai") {
            return true;
        }
    }

    // Check if a sibling "git-ai" exists in the same directory.
    // On Windows the installer copies git-ai.exe to git.exe (not a symlink or
    // hard-link), so same_file() would return false. A sibling git-ai.exe
    // existing is sufficient to identify this as the git-ai install directory.
    // On Unix, additionally verify both refer to the same underlying file
    // (hard-link / bind-mount) to avoid false-positives in environments where
    // a real git binary legitimately coexists with a git-ai symlink (e.g.
    // Docker images that compile git from source into /usr/local/bin).
    if let Some(parent) = path.parent() {
        #[cfg(windows)]
        let sibling = parent.join("git-ai.exe");
        #[cfg(not(windows))]
        let sibling = parent.join("git-ai");

        #[cfg(windows)]
        if sibling.exists() {
            return true;
        }
        #[cfg(not(windows))]
        if sibling.exists() && same_file(path, &sibling) {
            return true;
        }
    }

    false
}

/// Returns true if `p` is an executable git binary that is NOT git-ai.
/// Used by test infrastructure to probe for the real git binary independently
/// of `Config::get()` (which reads HOME and must not be called before HOME is isolated).
pub fn is_real_git_candidate(p: &Path) -> bool {
    is_executable(p) && !path_is_git_ai_binary(p)
}
