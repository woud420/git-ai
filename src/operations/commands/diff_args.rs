//! Argument parsing for the `git-ai diff` command.

use crate::error::GitAiError;
use crate::operations::commands::diff::{DiffCommandOptions, DiffFormat, DiffSpec, ParsedDiffArgs};

/// Parse the command-line arguments for `git-ai diff`.
pub fn parse_diff_args(args: &[String]) -> Result<ParsedDiffArgs, GitAiError> {
    let mut options = DiffCommandOptions::default();
    let mut positional_args: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                options.format = DiffFormat::Json;
                i += 1;
            }
            "--blame-deletions" => {
                options.blame_deletions = true;
                i += 1;
            }
            "--blame-deletions-since" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "--blame-deletions-since requires a value".to_string(),
                    ));
                }
                options.blame_deletions_since = Some(args[i + 1].clone());
                i += 2;
            }
            "--include-stats" => {
                options.include_stats = true;
                i += 1;
            }
            "--all-prompts" => {
                options.all_prompts = true;
                i += 1;
            }
            arg if arg.starts_with("--") => {
                return Err(GitAiError::Generic(format!("Unknown option: {}", arg)));
            }
            _ => {
                positional_args.push(args[i].as_str());
                i += 1;
            }
        }
    }

    if options.blame_deletions_since.is_some() && !options.blame_deletions {
        return Err(GitAiError::Generic(
            "--blame-deletions-since requires --blame-deletions".to_string(),
        ));
    }
    if options.include_stats && !matches!(options.format, DiffFormat::Json) {
        return Err(GitAiError::Generic(
            "--include-stats requires --json".to_string(),
        ));
    }
    if options.all_prompts && !matches!(options.format, DiffFormat::Json) {
        return Err(GitAiError::Generic(
            "--all-prompts requires --json".to_string(),
        ));
    }

    let spec = match positional_args.as_slice() {
        [] => {
            return Err(GitAiError::Generic(
                "diff requires a commit or commit range argument".to_string(),
            ));
        }
        [start, end] => {
            if start.contains("..") || end.contains("..") {
                return Err(GitAiError::Generic(
                    "Invalid diff arguments. Expected: <commit>, <commit1>..<commit2>, or <commit1> <commit2>".to_string(),
                ));
            }
            DiffSpec::TwoCommit((*start).to_string(), (*end).to_string())
        }
        [arg] => {
            if arg.contains("..") {
                let parts: Vec<&str> = arg.split("..").collect();
                if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                    DiffSpec::TwoCommit(parts[0].to_string(), parts[1].to_string())
                } else {
                    return Err(GitAiError::Generic(
                        "Invalid commit range format. Expected: <commit>..<commit>".to_string(),
                    ));
                }
            } else {
                DiffSpec::SingleCommit(positional_args[0].to_string())
            }
        }
        _ => {
            return Err(GitAiError::Generic(
                "Invalid diff arguments. Expected: <commit>, <commit1>..<commit2>, or <commit1> <commit2>".to_string(),
            ));
        }
    };

    if options.include_stats && matches!(spec, DiffSpec::TwoCommit(_, _)) {
        return Err(GitAiError::Generic(
            "--include-stats is only supported for single-commit diffs".to_string(),
        ));
    }
    if options.all_prompts && matches!(spec, DiffSpec::TwoCommit(_, _)) {
        return Err(GitAiError::Generic(
            "--all-prompts is only supported for single-commit diffs".to_string(),
        ));
    }

    Ok(ParsedDiffArgs { spec, options })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::commands::diff::{DiffFormat, DiffSpec};

    #[test]
    fn test_parse_diff_args_single_commit() {
        let args = vec!["abc123".to_string()];
        let parsed = parse_diff_args(&args).unwrap();

        match parsed.spec {
            DiffSpec::SingleCommit(sha) => {
                assert_eq!(sha, "abc123");
            }
            _ => panic!("Expected SingleCommit"),
        }

        assert!(matches!(
            parsed.options.format,
            DiffFormat::GitCompatibleTerminal
        ));
        assert!(!parsed.options.blame_deletions);
        assert!(parsed.options.blame_deletions_since.is_none());
        assert!(!parsed.options.include_stats);
        assert!(!parsed.options.all_prompts);
    }

    #[test]
    fn test_parse_diff_args_commit_range() {
        let args = vec!["abc123..def456".to_string()];
        let parsed = parse_diff_args(&args).unwrap();

        match parsed.spec {
            DiffSpec::TwoCommit(start, end) => {
                assert_eq!(start, "abc123");
                assert_eq!(end, "def456");
            }
            _ => panic!("Expected TwoCommit"),
        }
    }

    #[test]
    fn test_parse_diff_args_two_positional_commits() {
        let args = vec!["abc123".to_string(), "def456".to_string()];
        let parsed = parse_diff_args(&args).unwrap();

        match parsed.spec {
            DiffSpec::TwoCommit(start, end) => {
                assert_eq!(start, "abc123");
                assert_eq!(end, "def456");
            }
            _ => panic!("Expected TwoCommit"),
        }
    }

    #[test]
    fn test_parse_diff_args_two_positional_commits_with_json() {
        let args = vec![
            "abc123".to_string(),
            "def456".to_string(),
            "--json".to_string(),
        ];
        let parsed = parse_diff_args(&args).unwrap();

        match parsed.spec {
            DiffSpec::TwoCommit(start, end) => {
                assert_eq!(start, "abc123");
                assert_eq!(end, "def456");
            }
            _ => panic!("Expected TwoCommit"),
        }

        assert!(matches!(parsed.options.format, DiffFormat::Json));
    }

    #[test]
    fn test_parse_diff_args_include_stats_requires_json() {
        let args = vec!["abc123".to_string(), "--include-stats".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_include_stats_single_commit_json() {
        let args = vec![
            "abc123".to_string(),
            "--json".to_string(),
            "--include-stats".to_string(),
        ];
        let parsed = parse_diff_args(&args).unwrap();
        assert!(matches!(parsed.spec, DiffSpec::SingleCommit(_)));
        assert!(matches!(parsed.options.format, DiffFormat::Json));
        assert!(parsed.options.include_stats);
    }

    #[test]
    fn test_parse_diff_args_include_stats_rejects_ranges() {
        let args = vec![
            "abc123..def456".to_string(),
            "--json".to_string(),
            "--include-stats".to_string(),
        ];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_all_prompts_requires_json() {
        let args = vec!["abc123".to_string(), "--all-prompts".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_all_prompts_single_commit_json() {
        let args = vec![
            "abc123".to_string(),
            "--json".to_string(),
            "--all-prompts".to_string(),
        ];
        let parsed = parse_diff_args(&args).unwrap();
        assert!(matches!(parsed.spec, DiffSpec::SingleCommit(_)));
        assert!(matches!(parsed.options.format, DiffFormat::Json));
        assert!(parsed.options.all_prompts);
    }

    #[test]
    fn test_parse_diff_args_all_prompts_rejects_ranges() {
        let args = vec![
            "abc123..def456".to_string(),
            "--json".to_string(),
            "--all-prompts".to_string(),
        ];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_blame_deletions_flags() {
        let args = vec![
            "abc123".to_string(),
            "--blame-deletions".to_string(),
            "--blame-deletions-since".to_string(),
            "2 weeks ago".to_string(),
        ];
        let parsed = parse_diff_args(&args).unwrap();
        assert!(parsed.options.blame_deletions);
        assert_eq!(
            parsed.options.blame_deletions_since,
            Some("2 weeks ago".to_string())
        );
    }

    #[test]
    fn test_parse_diff_args_blame_deletions_since_requires_blame_deletions() {
        let args = vec![
            "abc123".to_string(),
            "--blame-deletions-since".to_string(),
            "2026-01-01".to_string(),
        ];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_too_many_positional_args() {
        let args = vec![
            "abc123".to_string(),
            "def456".to_string(),
            "ghi789".to_string(),
        ];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_only_json_flag() {
        let args = vec!["--json".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_diff_args_invalid_range() {
        let args = vec!["..".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());

        let args = vec!["abc..".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());

        let args = vec!["..def".to_string()];
        let result = parse_diff_args(&args);
        assert!(result.is_err());
    }
}
