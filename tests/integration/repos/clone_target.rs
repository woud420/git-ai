//! `git clone` target-directory parsing, used by the test harness to find the
//! repository a clone created.

/// Returns true if the given flag typically takes a value as the next argument.
/// This is a heuristic for common git command flags that take values.
fn is_flag_with_value(flag: &str) -> bool {
    matches!(
        flag,
        // Commit/merge message flags
        "-m" | "--message" |
        "-F" | "--file" |
        "-t" | "--template" |
        "-e" | "--edit" |
        "--author" | "--date" |
        // Merge strategy
        "-s" | "--strategy" |
        "-X" | "--strategy-option" |
        // Log/diff flags
        "--since" | "--until" | "--before" | "--after" |
        "--format" | "--pretty" |
        "-n" | "--max-count" |
        "--skip" |
        // Checkout/branch flags
        "-b" | "-B" |
        // Push/pull flags
        "-u" | "--set-upstream" |
        // Config flags
        "--config" |
        // Misc
        "--depth" | "--shallow-since"
    )
}

/// Extract the target directory from git clone command arguments.
/// Returns the directory where the repository was cloned to.
///
/// Logic:
/// - First non-option positional arg is the repository URL
/// - Second non-option positional arg (if present) is the target directory
/// - If no directory specified, derive from last component of URL (strip .git suffix)
pub fn extract_clone_target_directory(args: &[String]) -> Option<String> {
    let mut positional_args = Vec::new();
    let mut after_double_dash = false;
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if !after_double_dash {
            // Check for -- separator
            if arg == "--" {
                after_double_dash = true;
                i += 1;
                continue;
            }

            // Skip options that take a value
            if matches!(arg.as_str(), "-o" | "--origin") || is_flag_with_value(arg) {
                i += 2; // Skip both option and its value
                continue;
            }

            // Skip standalone options
            if arg.starts_with('-') {
                i += 1;
                continue;
            }
        }

        // This is a positional argument
        positional_args.push(arg.clone());
        i += 1;
    }

    // Need at least one positional arg (the repository URL)
    if positional_args.is_empty() {
        return None;
    }

    // If we have 2+ positional args, the second one is the target directory
    if positional_args.len() >= 2 {
        return Some(positional_args[1].clone());
    }

    // Otherwise, derive directory name from repository URL
    let repo_url = &positional_args[0];
    derive_directory_from_url(repo_url)
}

/// Derive the target directory name from a repository URL.
/// Mimics git's behavior of using the last path component, stripping .git suffix.
fn derive_directory_from_url(url: &str) -> Option<String> {
    // Remove trailing slashes and backslashes (Windows)
    let url = url.trim_end_matches(&['/', '\\'] as &[char]);

    // Extract the last path component (consider both / and \ for Windows paths)
    let last_sep = url.rfind(&['/', '\\'] as &[char]);
    let last_component = if let Some(pos) = last_sep {
        &url[pos + 1..]
    } else if let Some(pos) = url.rfind(':') {
        // Handle SCP-like syntax: user@host:path
        &url[pos + 1..]
    } else {
        url
    };

    if last_component.is_empty() {
        return None;
    }

    // Strip .git suffix if present
    let dir_name = last_component
        .strip_suffix(".git")
        .unwrap_or(last_component);

    if dir_name.is_empty() {
        None
    } else {
        Some(dir_name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_directory_from_url() {
        assert_eq!(
            derive_directory_from_url("https://github.com/user/repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_directory_from_url("https://github.com/user/repo"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_directory_from_url("git@github.com:user/repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_directory_from_url("user@host:path/to/repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_directory_from_url("/local/path/repo.git"),
            Some("repo".to_string())
        );
        // Windows backslash paths
        assert_eq!(
            derive_directory_from_url(r"C:\Users\runner\AppData\Local\Temp\repo"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_directory_from_url(r"C:\Users\runner\AppData\Local\Temp\repo.git"),
            Some("repo".to_string())
        );
        assert_eq!(
            derive_directory_from_url(r"\\?\C:\Temp\bare-repo"),
            Some("bare-repo".to_string())
        );
        // Trailing backslash
        assert_eq!(
            derive_directory_from_url("C:\\Users\\user\\repos\\repo.git\\"),
            Some("repo".to_string())
        );
    }

    #[test]
    fn test_extract_clone_target_directory() {
        // Explicit directory specified
        let args = vec![
            "https://github.com/user/repo.git".to_string(),
            "my-dir".to_string(),
        ];
        assert_eq!(
            extract_clone_target_directory(&args),
            Some("my-dir".to_string())
        );

        // Directory derived from URL
        let args = vec!["https://github.com/user/repo.git".to_string()];
        assert_eq!(
            extract_clone_target_directory(&args),
            Some("repo".to_string())
        );

        // With options
        let args = vec![
            "-b".to_string(),
            "main".to_string(),
            "https://github.com/user/repo.git".to_string(),
        ];
        assert_eq!(
            extract_clone_target_directory(&args),
            Some("repo".to_string())
        );

        // With options and explicit directory
        let args = vec![
            "-b".to_string(),
            "main".to_string(),
            "https://github.com/user/repo.git".to_string(),
            "my-dir".to_string(),
        ];
        assert_eq!(
            extract_clone_target_directory(&args),
            Some("my-dir".to_string())
        );

        // With --option=value syntax
        let args = vec![
            "--branch=main".to_string(),
            "https://github.com/user/repo.git".to_string(),
            "my-dir".to_string(),
        ];
        assert_eq!(
            extract_clone_target_directory(&args),
            Some("my-dir".to_string())
        );
    }
}
