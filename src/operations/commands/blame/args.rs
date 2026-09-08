use std::fs;
use std::io;

use chrono::DateTime;

use crate::error::GitAiError;

use super::GitAiBlameOptions;

pub fn parse_blame_args(args: &[String]) -> Result<(String, GitAiBlameOptions), GitAiError> {
    let mut options = GitAiBlameOptions::default();
    let mut file_path = None;
    let mut args = args.iter().peekable();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            // Line range options
            "-L" => {
                let range_str = next_value(&mut args, arg)?;
                if let Some((start, end)) = parse_line_range(range_str) {
                    options.line_ranges.push((start, end));
                } else {
                    return Err(GitAiError::Generic(format!(
                        "Invalid line range: {}",
                        range_str
                    )));
                }
            }

            // Output format options
            "--porcelain" => options.porcelain = true,
            "--line-porcelain" => {
                options.line_porcelain = true;
                options.porcelain = true; // Implies --porcelain
            }
            "--incremental" => options.incremental = true,
            "-f" | "--show-name" => options.show_name = true,
            "-n" | "--show-number" => options.show_number = true,
            "-e" | "--show-email" => options.show_email = true,
            "-s" => options.suppress_author = true,
            "--show-stats" => options.show_stats = true,

            // Commit display options
            "-l" => options.long_rev = true,
            "-t" => options.raw_timestamp = true,
            "--abbrev" => {
                if let Ok(n) = next_value(&mut args, arg)?.parse::<u32>() {
                    options.abbrev = Some(n);
                } else {
                    return Err(GitAiError::Generic(
                        "Invalid number for --abbrev".to_string(),
                    ));
                }
            }

            // Boundary options
            "-b" => options.blank_boundary = true,
            "--root" => options.show_root = true,

            // Movement detection options
            "-M" | "-C" => {
                if arg == "-M" {
                    options.detect_moves = true;
                } else {
                    options.detect_copies = (options.detect_copies + 1).min(3);
                }
                if let Some(threshold) = args.peek().and_then(|value| value.parse::<u32>().ok()) {
                    options.move_threshold = Some(threshold);
                    args.next();
                }
            }

            // Ignore options
            "--ignore-rev" => {
                options
                    .ignore_revs
                    .push(next_value(&mut args, arg)?.clone());
            }
            "--ignore-revs-file" => {
                options.ignore_revs_file = Some(next_value(&mut args, arg)?.clone());
            }
            "--no-ignore-revs-file" => {
                // Disable auto-detection of .git-blame-ignore-revs file
                options.no_ignore_revs_file = true;
            }

            // Color options
            "--color-lines" => options.color_lines = true,
            "--color-by-age" => options.color_by_age = true,

            // Progress options
            "--progress" => options.progress = true,

            // Date format
            "--date" => {
                options.date_format = Some(next_value(&mut args, arg)?.clone());
            }

            // Content options
            "--contents" => {
                let contents_arg = next_value(&mut args, arg)?;
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
            }

            // Revision options
            "--reverse" => {
                options.reverse = Some(next_value(&mut args, arg)?.clone());
            }
            "--first-parent" => options.first_parent = true,

            // Encoding
            "--encoding" => {
                options.encoding = Some(next_value(&mut args, arg)?.clone());
            }

            // Date filtering
            "--since" => {
                options.oldest_date = Some(
                    DateTime::parse_from_rfc3339(next_value(&mut args, arg)?).map_err(|e| {
                        GitAiError::Generic(format!("Invalid date format for --since: {}", e))
                    })?,
                );
            }
            // JSON output format
            "--json" => options.json = true,

            // Mark unknown authorship
            "--mark-unknown" => options.mark_unknown = true,

            // Show prompt hashes inline
            "--show-prompt" => options.show_prompt = true,

            // File path (non-option argument)
            arg if !arg.starts_with('-') => {
                if file_path.is_none() {
                    file_path = Some(arg.to_string());
                } else {
                    return Err(GitAiError::Generic(
                        "Multiple file paths specified".to_string(),
                    ));
                }
            }

            // Unknown option
            _ => {
                return Err(GitAiError::Generic(format!("Unknown option: {}", arg)));
            }
        }
    }

    let file_path =
        file_path.ok_or_else(|| GitAiError::Generic("No file path specified".to_string()))?;

    Ok((file_path, options))
}

fn next_value<'a>(
    args: &mut impl Iterator<Item = &'a String>,
    option: &str,
) -> Result<&'a String, GitAiError> {
    args.next()
        .ok_or_else(|| GitAiError::Generic(format!("Missing argument for {option}")))
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
