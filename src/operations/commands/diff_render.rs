//! Terminal rendering for `git-ai diff` (non-JSON output).
//!
//! Formats an annotated unified diff with ANSI colour and per-line attribution
//! badges for display in a terminal.

use crate::error::GitAiError;
use crate::model::authorship_log::HumanRecord;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::IsTerminal;

use crate::operations::commands::diff::{Attribution, DiffLineKey, LineSide};
use crate::operations::commands::diff_parsing::get_diff_sections_by_file;
use crate::operations::git::repository::Repository;

// ============================================================================
// Public entry point
// ============================================================================

#[allow(clippy::if_same_then_else)]
pub fn format_annotated_diff(
    repo: &Repository,
    from_commit: &str,
    to_commit: &str,
    attributions: &HashMap<DiffLineKey, Attribution>,
    humans: &BTreeMap<String, HumanRecord>,
    included_files: &HashSet<String>,
) -> Result<String, GitAiError> {
    let sections = get_diff_sections_by_file(repo, from_commit, to_commit)?;
    let use_color = std::io::stdout().is_terminal();
    let mut result = String::new();

    for (file_path, section_text) in sections {
        if !included_files.contains(&file_path) {
            continue;
        }

        let mut old_line_num = 0u32;
        let mut new_line_num = 0u32;
        let mut in_hunk = false;

        for line in section_text.lines() {
            if is_diff_header_line(line, in_hunk) {
                if line.starts_with("diff --git") {
                    in_hunk = false;
                }
                result.push_str(&format_line(
                    line,
                    LineType::DiffHeader,
                    use_color,
                    None,
                    humans,
                ));
            } else if line.starts_with("@@ ") {
                in_hunk = true;
                if let Some((old_start, new_start)) = parse_hunk_header_for_line_nums(line) {
                    old_line_num = old_start;
                    new_line_num = new_start;
                }
                result.push_str(&format_line(
                    line,
                    LineType::HunkHeader,
                    use_color,
                    None,
                    humans,
                ));
            } else if in_hunk && line.starts_with('-') {
                let key = DiffLineKey {
                    file: file_path.clone(),
                    line: old_line_num,
                    side: LineSide::Old,
                };
                let attribution = attributions.get(&key);
                result.push_str(&format_line(
                    line,
                    LineType::Deletion,
                    use_color,
                    attribution,
                    humans,
                ));
                old_line_num += 1;
            } else if in_hunk && line.starts_with('+') {
                let key = DiffLineKey {
                    file: file_path.clone(),
                    line: new_line_num,
                    side: LineSide::New,
                };
                let attribution = attributions.get(&key);
                result.push_str(&format_line(
                    line,
                    LineType::Addition,
                    use_color,
                    attribution,
                    humans,
                ));
                new_line_num += 1;
            } else if in_hunk && line.starts_with(' ') {
                result.push_str(&format_line(
                    line,
                    LineType::Context,
                    use_color,
                    None,
                    humans,
                ));
                old_line_num += 1;
                new_line_num += 1;
            } else if line.starts_with("Binary files") {
                result.push_str(&format_line(
                    line,
                    LineType::Binary,
                    use_color,
                    None,
                    humans,
                ));
            } else {
                result.push_str(&format_line(
                    line,
                    LineType::Context,
                    use_color,
                    None,
                    humans,
                ));
            }
        }
    }

    Ok(result)
}

// ============================================================================
// Internal helpers
// ============================================================================

fn is_diff_header_line(line: &str, in_hunk: bool) -> bool {
    line.starts_with("diff --git")
        || line.starts_with("index ")
        || (!in_hunk && (line.starts_with("--- ") || line.starts_with("+++ ")))
}

fn parse_hunk_header_for_line_nums(line: &str) -> Option<(u32, u32)> {
    // Parse @@ -old_start,old_count +new_start,new_count @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let old_part = parts[1];
    let new_part = parts[2];

    let old_str = old_part.strip_prefix('-')?;
    let old_start = if let Some((start_str, _)) = old_str.split_once(',') {
        start_str.parse::<u32>().ok()?
    } else {
        old_str.parse::<u32>().ok()?
    };

    let new_str = new_part.strip_prefix('+')?;
    let new_start = if let Some((start_str, _)) = new_str.split_once(',') {
        start_str.parse::<u32>().ok()?
    } else {
        new_str.parse::<u32>().ok()?
    };

    Some((old_start, new_start))
}

#[derive(Debug)]
enum LineType {
    DiffHeader,
    HunkHeader,
    Addition,
    Deletion,
    Context,
    Binary,
}

fn format_line(
    line: &str,
    line_type: LineType,
    use_color: bool,
    attribution: Option<&Attribution>,
    humans: &BTreeMap<String, HumanRecord>,
) -> String {
    let annotation = if let Some(attr) = attribution {
        format_attribution(attr, humans)
    } else {
        String::new()
    };

    if use_color {
        match line_type {
            LineType::DiffHeader => {
                format!("\x1b[1m{}\x1b[0m\n", line) // Bold
            }
            LineType::HunkHeader => {
                format!("\x1b[36m{}\x1b[0m\n", line) // Cyan
            }
            LineType::Addition => {
                if annotation.is_empty() {
                    format!("\x1b[32m{}\x1b[0m\n", line) // Green
                } else {
                    format!("\x1b[32m{}\x1b[0m  \x1b[2m{}\x1b[0m\n", line, annotation) // Green + dim annotation
                }
            }
            LineType::Deletion => {
                if annotation.is_empty() {
                    format!("\x1b[31m{}\x1b[0m\n", line) // Red
                } else {
                    format!("\x1b[31m{}\x1b[0m  \x1b[2m{}\x1b[0m\n", line, annotation) // Red + dim annotation
                }
            }
            LineType::Context | LineType::Binary => {
                format!("{}\n", line)
            }
        }
    } else {
        // No color
        if annotation.is_empty() {
            format!("{}\n", line)
        } else {
            format!("{}  {}\n", line, annotation)
        }
    }
}

fn format_attribution(attribution: &Attribution, humans: &BTreeMap<String, HumanRecord>) -> String {
    match attribution {
        Attribution::Ai(tool) => format!("🤖{}", tool),
        Attribution::Human(human_id) => {
            // Resolve human_id (h_-prefixed hash) to actual author name.
            if let Some(human_record) = humans.get(human_id) {
                format!("👤{}", human_record.author)
            } else {
                // Fallback to showing the ID if not found in humans map.
                format!("👤{}", human_id)
            }
        }
        Attribution::NoData => "[no-data]".to_string(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authorship_log::HumanRecord;

    #[test]
    fn test_format_attribution_ai() {
        let humans = BTreeMap::new();
        let attr = Attribution::Ai("cursor".to_string());
        assert_eq!(format_attribution(&attr, &humans), "🤖cursor");

        let attr = Attribution::Ai("claude".to_string());
        assert_eq!(format_attribution(&attr, &humans), "🤖claude");
    }

    #[test]
    fn test_format_attribution_human() {
        let mut humans = BTreeMap::new();
        humans.insert(
            "h_alice123".to_string(),
            HumanRecord {
                author: "alice".to_string(),
            },
        );
        humans.insert(
            "h_bob456".to_string(),
            HumanRecord {
                author: "bob@example.com".to_string(),
            },
        );

        let attr = Attribution::Human("h_alice123".to_string());
        assert_eq!(format_attribution(&attr, &humans), "👤alice");

        let attr = Attribution::Human("h_bob456".to_string());
        assert_eq!(format_attribution(&attr, &humans), "👤bob@example.com");

        // Fallback when human_id not in map.
        let attr = Attribution::Human("h_unknown".to_string());
        assert_eq!(format_attribution(&attr, &humans), "👤h_unknown");
    }

    #[test]
    fn test_format_attribution_no_data() {
        let humans = BTreeMap::new();
        let attr = Attribution::NoData;
        assert_eq!(format_attribution(&attr, &humans), "[no-data]");
    }

    #[test]
    fn test_is_diff_header_line_respects_hunk_state() {
        assert!(is_diff_header_line("diff --git a/f b/f", false));
        assert!(is_diff_header_line("index abc..def 100644", false));
        assert!(is_diff_header_line("--- a/file.txt", false));
        assert!(is_diff_header_line("+++ b/file.txt", false));
        assert!(!is_diff_header_line("--- content line", true));
        assert!(!is_diff_header_line("+++ content line", true));
    }

    #[test]
    fn test_parse_hunk_header_for_line_nums() {
        let line = "@@ -10,5 +20,3 @@ context";
        let result = parse_hunk_header_for_line_nums(line).unwrap();
        assert_eq!(result, (10, 20));
    }

    #[test]
    fn test_parse_hunk_header_for_line_nums_single_line() {
        let line = "@@ -10 +20,3 @@ context";
        let result = parse_hunk_header_for_line_nums(line).unwrap();
        assert_eq!(result, (10, 20));

        let line = "@@ -10,5 +20 @@ context";
        let result = parse_hunk_header_for_line_nums(line).unwrap();
        assert_eq!(result, (10, 20));
    }

    #[test]
    fn test_parse_hunk_header_for_line_nums_invalid() {
        let line = "not a hunk header";
        let result = parse_hunk_header_for_line_nums(line);
        assert!(result.is_none());

        let line = "@@ invalid @@";
        let result = parse_hunk_header_for_line_nums(line);
        assert!(result.is_none());
    }
}
