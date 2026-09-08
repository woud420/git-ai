use std::fs;
use std::io;

use chrono::DateTime;

use crate::error::GitAiError;

use super::GitAiBlameOptions;

pub fn parse_blame_args(args: &[String]) -> Result<(String, GitAiBlameOptions), GitAiError> {
    let mut options = GitAiBlameOptions::default();
    let mut file_path = None;
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            // Line range options
            "-L" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic("Missing argument for -L".to_string()));
                }
                let range_str = &args[i + 1];
                if let Some((start, end)) = parse_line_range(range_str) {
                    options.line_ranges.push((start, end));
                } else {
                    return Err(GitAiError::Generic(format!(
                        "Invalid line range: {}",
                        range_str
                    )));
                }
                i += 2;
            }

            // Output format options
            "--porcelain" => {
                options.porcelain = true;
                i += 1;
            }
            "--line-porcelain" => {
                options.line_porcelain = true;
                options.porcelain = true; // Implies --porcelain
                i += 1;
            }
            "--incremental" => {
                options.incremental = true;
                i += 1;
            }
            "-f" | "--show-name" => {
                options.show_name = true;
                i += 1;
            }
            "-n" | "--show-number" => {
                options.show_number = true;
                i += 1;
            }
            "-e" | "--show-email" => {
                options.show_email = true;
                i += 1;
            }
            "-s" => {
                options.suppress_author = true;
                i += 1;
            }
            "--show-stats" => {
                options.show_stats = true;
                i += 1;
            }

            // Commit display options
            "-l" => {
                options.long_rev = true;
                i += 1;
            }
            "-t" => {
                options.raw_timestamp = true;
                i += 1;
            }
            "--abbrev" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --abbrev".to_string(),
                    ));
                }
                if let Ok(n) = args[i + 1].parse::<u32>() {
                    options.abbrev = Some(n);
                } else {
                    return Err(GitAiError::Generic(
                        "Invalid number for --abbrev".to_string(),
                    ));
                }
                i += 2;
            }

            // Boundary options
            "-b" => {
                options.blank_boundary = true;
                i += 1;
            }
            "--root" => {
                options.show_root = true;
                i += 1;
            }

            // Movement detection options
            "-M" => {
                options.detect_moves = true;
                if i + 1 < args.len() {
                    if let Ok(threshold) = args[i + 1].parse::<u32>() {
                        options.move_threshold = Some(threshold);
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
            "-C" => {
                options.detect_copies = (options.detect_copies + 1).min(3);
                if i + 1 < args.len() {
                    if let Ok(threshold) = args[i + 1].parse::<u32>() {
                        options.move_threshold = Some(threshold);
                        i += 2;
                    } else {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }

            // Ignore options
            "--ignore-rev" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --ignore-rev".to_string(),
                    ));
                }
                options.ignore_revs.push(args[i + 1].clone());
                i += 2;
            }
            "--ignore-revs-file" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --ignore-revs-file".to_string(),
                    ));
                }
                options.ignore_revs_file = Some(args[i + 1].clone());
                i += 2;
            }
            "--no-ignore-revs-file" => {
                // Disable auto-detection of .git-blame-ignore-revs file
                options.no_ignore_revs_file = true;
                i += 1;
            }

            // Color options
            "--color-lines" => {
                options.color_lines = true;
                i += 1;
            }
            "--color-by-age" => {
                options.color_by_age = true;
                i += 1;
            }

            // Progress options
            "--progress" => {
                options.progress = true;
                i += 1;
            }

            // Date format
            "--date" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --date".to_string(),
                    ));
                }
                options.date_format = Some(args[i + 1].clone());
                i += 2;
            }

            // Content options
            "--contents" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --contents".to_string(),
                    ));
                }
                let contents_arg = &args[i + 1];
                options.contents_file = Some(contents_arg.clone());

                // Read the contents now - either from stdin or from a file
                let data = if contents_arg == "-" {
                    // Read from stdin
                    use std::io::Read;
                    let mut buffer = Vec::new();
                    io::stdin().read_to_end(&mut buffer).map_err(|e| {
                        GitAiError::Generic(format!("Failed to read from stdin: {}", e))
                    })?;
                    buffer
                } else {
                    // Read from file
                    fs::read(contents_arg).map_err(|e| {
                        GitAiError::Generic(format!(
                            "Failed to read contents file '{}': {}",
                            contents_arg, e
                        ))
                    })?
                };
                options.contents_data = Some(data);
                i += 2;
            }

            // Revision options
            "--reverse" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --reverse".to_string(),
                    ));
                }
                options.reverse = Some(args[i + 1].clone());
                i += 2;
            }
            "--first-parent" => {
                options.first_parent = true;
                i += 1;
            }

            // Encoding
            "--encoding" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --encoding".to_string(),
                    ));
                }
                options.encoding = Some(args[i + 1].clone());
                i += 2;
            }

            // Date filtering
            "--since" => {
                if i + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "Missing argument for --since".to_string(),
                    ));
                }
                options.oldest_date =
                    Some(DateTime::parse_from_rfc3339(&args[i + 1]).map_err(|e| {
                        GitAiError::Generic(format!("Invalid date format for --since: {}", e))
                    })?);
                i += 2;
            }
            // JSON output format
            "--json" => {
                options.json = true;
                i += 1;
            }

            // Mark unknown authorship
            "--mark-unknown" => {
                options.mark_unknown = true;
                i += 1;
            }

            // Show prompt hashes inline
            "--show-prompt" => {
                options.show_prompt = true;
                i += 1;
            }

            // File path (non-option argument)
            arg if !arg.starts_with('-') => {
                if file_path.is_none() {
                    file_path = Some(arg.to_string());
                } else {
                    return Err(GitAiError::Generic(
                        "Multiple file paths specified".to_string(),
                    ));
                }
                i += 1;
            }

            // Unknown option
            _ => {
                return Err(GitAiError::Generic(format!("Unknown option: {}", args[i])));
            }
        }
    }

    let file_path =
        file_path.ok_or_else(|| GitAiError::Generic("No file path specified".to_string()))?;

    Ok((file_path, options))
}

pub(super) fn parse_line_range(range_str: &str) -> Option<(u32, u32)> {
    if let Some(dash_pos) = range_str.find(',') {
        let start_str = &range_str[..dash_pos];
        let end_str = &range_str[dash_pos + 1..];

        if let (Ok(start), Ok(end)) = (start_str.parse::<u32>(), end_str.parse::<u32>()) {
            return Some((start, end));
        }
    } else if let Ok(line) = range_str.parse::<u32>() {
        return Some((line, line));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<(String, GitAiBlameOptions), GitAiError> {
        parse_blame_args(&args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>())
    }

    fn assert_error(args: &[&str], expected: &str) {
        assert_eq!(
            parse(args).unwrap_err().to_string(),
            format!("Generic error: {expected}")
        );
    }

    #[rstest::rstest]
    fn missing_values_report_the_flag(
        #[values(
            "-L",
            "--abbrev",
            "--ignore-rev",
            "--ignore-revs-file",
            "--date",
            "--contents",
            "--reverse",
            "--encoding",
            "--since"
        )]
        flag: &str,
    ) {
        assert_error(&[flag], &format!("Missing argument for {flag}"));
    }

    #[rstest::rstest]
    #[case(&[], "No file path specified")]
    #[case(&["--abbrev", "--root", "file"], "Invalid number for --abbrev")]
    #[case(&["-L", "--json", "file"], "Invalid line range: --json")]
    #[case(&["--unknown", "--abbrev"], "Unknown option: --unknown")]
    #[case(&["file", "other", "--abbrev"], "Multiple file paths specified")]
    #[case(&["--help"], "Unknown option: --help")]
    #[case(&["--abbrev=7"], "Unknown option: --abbrev=7")]
    #[case(&["-fn"], "Unknown option: -fn")]
    #[case(&["--"], "Unknown option: --")]
    fn errors_follow_argument_order(#[case] args: &[&str], #[case] expected: &str) {
        assert_error(args, expected);
    }

    #[test]
    fn required_string_values_consume_options_and_repeats_keep_the_last_value() {
        let (_, options) = parse(&[
            "--ignore-rev",
            "--json",
            "--ignore-rev",
            "",
            "--ignore-revs-file",
            "--json",
            "--date",
            "--root",
            "--date",
            "last",
            "--encoding",
            "--help",
            "--reverse",
            "-x",
            "--abbrev",
            "1",
            "--abbrev",
            "7",
            "file",
        ])
        .unwrap();
        assert_eq!(options.ignore_revs, ["--json", ""]);
        assert_eq!(options.ignore_revs_file.as_deref(), Some("--json"));
        assert_eq!(options.date_format.as_deref(), Some("last"));
        assert_eq!(options.encoding.as_deref(), Some("--help"));
        assert_eq!(options.reverse.as_deref(), Some("-x"));
        assert_eq!(options.abbrev, Some(7));
        assert!(!options.json && !options.show_root);
    }

    #[test]
    fn movement_thresholds_only_consume_numbers_and_ranges_keep_order() {
        for flag in ["-M", "-C"] {
            for file in ["file", "4294967296"] {
                let (path, options) = parse(&[flag, file]).unwrap();
                assert_eq!(path, file);
                assert_eq!(options.move_threshold, None);
            }
        }
        let (_, options) = parse(&[
            "-C", "1", "-C", "2", "-C", "3", "-C", "-M", "4", "-L", "3,2", "-L", "1", "file",
        ])
        .unwrap();
        assert_eq!(options.detect_copies, 3);
        assert!(options.detect_moves);
        assert_eq!(options.move_threshold, Some(4));
        assert_eq!(options.line_ranges, [(3, 2), (1, 1)]);
    }

    #[test]
    fn contents_are_read_during_parsing_before_later_errors() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("contents");
        fs::write(&path, b"\0\xff\n").unwrap();
        let (_, options) = parse(&["--contents", path.to_str().unwrap(), "file"]).unwrap();
        assert_eq!(
            options.contents_data.as_deref(),
            Some(b"\0\xff\n".as_slice())
        );
        assert_eq!(options.contents_file.as_deref(), path.to_str());
        let missing = temp.path().join("missing");
        let missing = missing.to_str().unwrap();
        let expected = format!(
            "Failed to read contents file '{missing}': {}",
            fs::read(missing).unwrap_err()
        );
        assert_error(&["--contents", missing, "--unknown"], &expected);
        assert_error(
            &["--unknown", "--contents", missing],
            "Unknown option: --unknown",
        );
    }
}
