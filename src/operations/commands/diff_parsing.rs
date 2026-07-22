//! Raw diff text retrieval and hunk parsing.
//!
//! This module handles invoking `git diff` and parsing the unified-diff output
//! into [`DiffHunk`] values.  It does not perform any attribution work.

use crate::clients::git_cli::{InternalGitProfile, exec_git_with_profile};
use crate::error::GitAiError;
use crate::operations::commands::diff_header_paths::{
    parse_diff_git_header_paths, parse_new_file_path_from_plus_header_line,
    parse_old_file_path_from_minus_header_line,
};
use crate::operations::git::repository::Repository;

use crate::operations::commands::diff::{DiffHunk, DiffLineKey, LineSide};

// ============================================================================
// Public entry points
// ============================================================================

/// Get diff hunks between two commits with absolute line numbers for each change.
pub fn get_diff_with_line_numbers(
    repo: &Repository,
    from: &str,
    to: &str,
) -> Result<Vec<DiffHunk>, GitAiError> {
    let diff_text = get_diff_text(repo, from, to, true)?;
    parse_diff_hunks(&diff_text)
}

/// Get the unified diff split by file path, including header lines.
///
/// Returns a list of `(file_path, diff_text)` pairs where `diff_text` is the
/// full diff section (header + hunks) for that file.  Binary-file sections are
/// excluded.
pub(crate) fn get_diff_sections_by_file(
    repo: &Repository,
    from_commit: &str,
    to_commit: &str,
) -> Result<Vec<(String, String)>, GitAiError> {
    let diff_text = get_diff_text(repo, from_commit, to_commit, false)?;
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut current_file = String::new();
    let mut current_diff = String::new();
    let mut current_old_file: Option<String> = None;
    let mut in_hunk = false;

    let flush_section = |sections: &mut Vec<(String, String)>,
                         current_file: &mut String,
                         current_diff: &mut String| {
        if !current_file.is_empty() && !current_diff.is_empty() {
            sections.push((current_file.clone(), current_diff.clone()));
        }
        current_file.clear();
        current_diff.clear();
    };

    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            flush_section(&mut sections, &mut current_file, &mut current_diff);
            if let Some((old_file, new_file)) = parse_diff_git_header_paths(line) {
                current_old_file = Some(old_file);
                current_file = new_file;
            } else {
                current_old_file = None;
                current_file.clear();
            }
            in_hunk = false;
            current_diff.push_str(line);
            current_diff.push('\n');
            continue;
        }

        if current_diff.is_empty() {
            continue;
        }

        current_diff.push_str(line);
        current_diff.push('\n');

        if line.starts_with("@@ ") {
            in_hunk = true;
            continue;
        }

        if !in_hunk {
            if let Some(path_opt) = parse_old_file_path_from_minus_header_line(line) {
                current_old_file = path_opt.clone();
                if current_file.is_empty() {
                    current_file = path_opt.unwrap_or_default();
                }
                continue;
            }

            if let Some(path_opt) = parse_new_file_path_from_plus_header_line(line) {
                current_file = path_opt
                    .or_else(|| current_old_file.clone())
                    .unwrap_or_default();
                continue;
            }
        }
    }

    flush_section(&mut sections, &mut current_file, &mut current_diff);

    // Exclude binary files — git emits "Binary files ... differ" lines for
    // these and they carry no useful text hunks.
    sections.retain(|(_, section_text)| !is_binary_diff_section(section_text));

    Ok(sections)
}

/// Returns `true` when a diff section produced by git describes a binary file.
pub(crate) fn is_binary_diff_section(section_text: &str) -> bool {
    section_text
        .lines()
        .any(|line| line.starts_with("Binary files"))
}

// ============================================================================
// Internal helpers
// ============================================================================

pub(super) fn get_diff_text(
    repo: &Repository,
    from: &str,
    to: &str,
    zero_context: bool,
) -> Result<String, GitAiError> {
    let mut args = repo.global_args_for_exec();
    args.push("diff".to_string());
    if zero_context {
        args.push("-U0".to_string()); // No context lines, just changes
    }
    // Use permissive rename detection so rename+edit commits are represented
    // as renames with edit hunks instead of delete/add file pairs.
    args.push("--find-renames=1%".to_string());
    args.push("--no-color".to_string());
    args.push(from.to_string());
    args.push(to.to_string());

    let output = exec_git_with_profile(&args, InternalGitProfile::PatchParse)?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(super) fn parse_diff_hunks(diff_text: &str) -> Result<Vec<DiffHunk>, GitAiError> {
    let mut hunks = Vec::new();
    let mut current_old_file: Option<String> = None;
    let mut current_file = String::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line_cursor = 0u32;
    let mut new_line_cursor = 0u32;

    let flush_current_hunk = |hunks: &mut Vec<DiffHunk>, current_hunk: &mut Option<DiffHunk>| {
        if let Some(hunk) = current_hunk.take() {
            hunks.push(hunk);
        }
    };

    for line in diff_text.lines() {
        if line.starts_with("diff --git ") {
            flush_current_hunk(&mut hunks, &mut current_hunk);
            if let Some((old_file, new_file)) = parse_diff_git_header_paths(line) {
                current_old_file = Some(old_file);
                current_file = new_file;
            } else {
                current_old_file = None;
                current_file.clear();
            }
            continue;
        }

        if current_hunk.is_none() {
            if let Some(path_opt) = parse_old_file_path_from_minus_header_line(line) {
                current_old_file = path_opt;
                if current_file.is_empty() {
                    current_file = current_old_file.clone().unwrap_or_default();
                }
                continue;
            }

            if let Some(path_opt) = parse_new_file_path_from_plus_header_line(line) {
                current_file = path_opt
                    .or_else(|| current_old_file.clone())
                    .unwrap_or_default();
                continue;
            }
        }

        if line.starts_with("@@ ") {
            flush_current_hunk(&mut hunks, &mut current_hunk);
            let old_file_path = current_old_file
                .as_deref()
                .filter(|old_path| *old_path != current_file.as_str());
            if let Some(mut hunk) = parse_hunk_line(line, &current_file, old_file_path)? {
                old_line_cursor = hunk.old_start;
                new_line_cursor = hunk.new_start;
                hunk.deleted_lines.clear();
                hunk.added_lines.clear();
                current_hunk = Some(hunk);
            }
            continue;
        }

        if let Some(hunk) = current_hunk.as_mut() {
            if let Some(stripped) = line.strip_prefix('-') {
                hunk.deleted_lines.push(old_line_cursor);
                hunk.deleted_contents.push(stripped.to_string());
                old_line_cursor += 1;
            } else if let Some(stripped) = line.strip_prefix('+') {
                hunk.added_lines.push(new_line_cursor);
                hunk.added_contents.push(stripped.to_string());
                new_line_cursor += 1;
            } else if line.starts_with(' ') {
                old_line_cursor += 1;
                new_line_cursor += 1;
            }
        }
    }

    flush_current_hunk(&mut hunks, &mut current_hunk);
    Ok(hunks)
}

pub(super) fn parse_hunk_line(
    line: &str,
    file_path: &str,
    old_file_path: Option<&str>,
) -> Result<Option<DiffHunk>, GitAiError> {
    // Parse hunk header format: @@ -old_start,old_count +new_start,new_count @@
    // Also handles: @@ -old_start +new_start,new_count @@ (single line deletion)
    // Also handles: @@ -old_start,old_count +new_start @@ (single line addition)

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return Ok(None);
    }

    let old_part = parts[1]; // e.g., "-10,3" or "-10"
    let new_part = parts[2]; // e.g., "+15,5" or "+15"

    // Parse old part
    let (old_start, old_count) = if let Some(old_str) = old_part.strip_prefix('-') {
        if let Some((start_str, count_str)) = old_str.split_once(',') {
            let start: u32 = start_str.parse().unwrap_or(0);
            let count: u32 = count_str.parse().unwrap_or(0);
            (start, count)
        } else {
            let start: u32 = old_str.parse().unwrap_or(0);
            (start, 1)
        }
    } else {
        (0, 0)
    };

    // Parse new part
    let (new_start, new_count) = if let Some(new_str) = new_part.strip_prefix('+') {
        if let Some((start_str, count_str)) = new_str.split_once(',') {
            let start: u32 = start_str.parse().unwrap_or(0);
            let count: u32 = count_str.parse().unwrap_or(0);
            (start, count)
        } else {
            let start: u32 = new_str.parse().unwrap_or(0);
            (start, 1)
        }
    } else {
        (0, 0)
    };

    // Build line number lists
    let deleted_lines: Vec<u32> = if old_count > 0 {
        (old_start..old_start + old_count).collect()
    } else {
        Vec::new()
    };

    let added_lines: Vec<u32> = if new_count > 0 {
        (new_start..new_start + new_count).collect()
    } else {
        Vec::new()
    };

    Ok(Some(DiffHunk {
        file_path: file_path.to_string(),
        old_file_path: old_file_path.map(ToString::to_string),
        old_start,
        old_count,
        new_start,
        new_count,
        deleted_lines,
        added_lines,
        deleted_contents: Vec::new(),
        added_contents: Vec::new(),
    }))
}

/// Build a map from [`DiffLineKey`] to the line's text content.
pub(super) fn build_line_content_map(
    hunks: &[DiffHunk],
) -> std::collections::HashMap<DiffLineKey, String> {
    let mut content_map = std::collections::HashMap::new();

    for hunk in hunks {
        for (line, content) in hunk.deleted_lines.iter().zip(hunk.deleted_contents.iter()) {
            content_map.insert(
                DiffLineKey {
                    file: hunk.file_path.clone(),
                    line: *line,
                    side: LineSide::Old,
                },
                content.clone(),
            );
        }
        for (line, content) in hunk.added_lines.iter().zip(hunk.added_contents.iter()) {
            content_map.insert(
                DiffLineKey {
                    file: hunk.file_path.clone(),
                    line: *line,
                    side: LineSide::New,
                },
                content.clone(),
            );
        }
    }

    content_map
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hunk_line_basic() {
        let line = "@@ -10,3 +15,5 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs", None).unwrap().unwrap();

        assert_eq!(result.file_path, "test.rs");
        assert_eq!(result.old_file_path, None);
        assert_eq!(result.old_start, 10);
        assert_eq!(result.old_count, 3);
        assert_eq!(result.new_start, 15);
        assert_eq!(result.new_count, 5);
        assert_eq!(result.deleted_lines, vec![10, 11, 12]);
        assert_eq!(result.added_lines, vec![15, 16, 17, 18, 19]);
    }

    #[test]
    fn test_parse_hunk_line_single_line_deletion() {
        let line = "@@ -10 +10,2 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs", None).unwrap().unwrap();

        assert_eq!(result.old_start, 10);
        assert_eq!(result.old_count, 1);
        assert_eq!(result.new_start, 10);
        assert_eq!(result.new_count, 2);
        assert_eq!(result.deleted_lines, vec![10]);
        assert_eq!(result.added_lines, vec![10, 11]);
    }

    #[test]
    fn test_parse_hunk_line_single_line_addition() {
        let line = "@@ -10,2 +10 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs", None).unwrap().unwrap();

        assert_eq!(result.old_start, 10);
        assert_eq!(result.old_count, 2);
        assert_eq!(result.new_start, 10);
        assert_eq!(result.new_count, 1);
        assert_eq!(result.deleted_lines, vec![10, 11]);
        assert_eq!(result.added_lines, vec![10]);
    }

    #[test]
    fn test_parse_hunk_line_pure_addition() {
        let line = "@@ -0,0 +1,3 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs", None).unwrap().unwrap();

        assert_eq!(result.old_start, 0);
        assert_eq!(result.old_count, 0);
        assert_eq!(result.new_start, 1);
        assert_eq!(result.new_count, 3);
        assert_eq!(result.deleted_lines.len(), 0);
        assert_eq!(result.added_lines, vec![1, 2, 3]);
    }

    #[test]
    fn test_parse_hunk_line_pure_deletion() {
        let line = "@@ -5,3 +0,0 @@ fn main() {";
        let result = parse_hunk_line(line, "test.rs", None).unwrap().unwrap();

        assert_eq!(result.old_start, 5);
        assert_eq!(result.old_count, 3);
        assert_eq!(result.new_start, 0);
        assert_eq!(result.new_count, 0);
        assert_eq!(result.deleted_lines, vec![5, 6, 7]);
        assert_eq!(result.added_lines.len(), 0);
    }

    #[test]
    fn test_parse_diff_hunks_multiple_files() {
        let diff_text = r#"diff --git a/file1.rs b/file1.rs
index abc123..def456 100644
--- a/file1.rs
+++ b/file1.rs
@@ -10,2 +10,3 @@ fn main() {
diff --git a/file2.rs b/file2.rs
index 111222..333444 100644
--- a/file2.rs
+++ b/file2.rs
@@ -5,1 +5,2 @@ fn test() {
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file_path, "file1.rs");
        assert_eq!(result[1].file_path, "file2.rs");
    }

    #[test]
    fn test_parse_diff_hunks_empty() {
        let diff_text = "";
        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_parse_diff_hunks_no_prefix_paths() {
        let diff_text = r#"diff --git file1.rs file1.rs
index abc123..def456 100644
--- file1.rs
+++ file1.rs
@@ -1,0 +1,1 @@
+fn added() {}
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, "file1.rs");
    }

    #[test]
    fn test_parse_diff_hunks_custom_prefix_paths() {
        let diff_text = r#"diff --git SRC/file1.rs DST/file1.rs
index abc123..def456 100644
--- SRC/file1.rs
+++ DST/file1.rs
@@ -1,0 +1,1 @@
+fn added() {}
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, "DST/file1.rs");
        assert_eq!(result[0].old_file_path, Some("SRC/file1.rs".to_string()));
    }

    #[test]
    fn test_parse_diff_hunks_rename_tracks_old_file_path() {
        let diff_text = r#"diff --git a/old_name.txt b/new_name.txt
similarity index 62%
rename from old_name.txt
rename to new_name.txt
index 7f4f5e8..1c84817 100644
--- a/old_name.txt
+++ b/new_name.txt
@@ -1,3 +1,2 @@
 keep
-drop-me
 tail
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, "new_name.txt");
        assert_eq!(result[0].old_file_path, Some("old_name.txt".to_string()));
    }

    #[test]
    fn test_parse_diff_hunks_preserves_header_like_content_inside_hunk() {
        let diff_text = r#"diff --git a/query.sql b/query.sql
index abc123..def456 100644
--- a/query.sql
+++ b/query.sql
@@ -10,3 +10,3 @@
--- old sql comment
-WHERE id = 1;
+++ new marker
+WHERE id = 2;
 SELECT * FROM users;
@@ -30,1 +30,1 @@
-regular old
+regular new
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].file_path, "query.sql");
        assert_eq!(result[0].deleted_lines, vec![10, 11]);
        assert_eq!(
            result[0].deleted_contents,
            vec!["-- old sql comment", "WHERE id = 1;"]
        );
        assert_eq!(result[0].added_lines, vec![10, 11]);
        assert_eq!(
            result[0].added_contents,
            vec!["++ new marker", "WHERE id = 2;"]
        );

        assert_eq!(result[1].file_path, "query.sql");
        assert_eq!(result[1].deleted_lines, vec![30]);
        assert_eq!(result[1].added_lines, vec![30]);
    }

    #[test]
    fn test_parse_diff_hunks_preserves_plus_plus_plus_content_inside_hunk() {
        let diff_text = r#"diff --git a/script.lua b/script.lua
index abc123..def456 100644
--- a/script.lua
+++ b/script.lua
@@ -41,0 +42,2 @@
+++ section marker
+print("hello")
"#;

        let result = parse_diff_hunks(diff_text).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_path, "script.lua");
        assert_eq!(result[0].added_lines, vec![42, 43]);
        assert_eq!(
            result[0].added_contents,
            vec!["++ section marker", "print(\"hello\")"]
        );
    }

    #[test]
    fn test_parse_diff_git_header_paths_standard_and_quoted() {
        use crate::operations::commands::diff_header_paths::parse_diff_git_header_paths;
        let parsed = parse_diff_git_header_paths("diff --git a/src/lib.rs b/src/lib.rs")
            .expect("standard diff header should parse");
        assert_eq!(parsed, ("src/lib.rs".to_string(), "src/lib.rs".to_string()));

        let parsed = parse_diff_git_header_paths(r#"diff --git "a/my file.rs" "b/my file.rs""#)
            .expect("quoted diff header should parse");
        assert_eq!(parsed, ("my file.rs".to_string(), "my file.rs".to_string()));
    }

    #[test]
    fn test_is_binary_diff_section_detects_binary() {
        let section = "diff --git a/image.png b/image.png\nnew file mode 100644\nindex 0000000..abc1234\nBinary files /dev/null and b/image.png differ\n";
        assert!(is_binary_diff_section(section));
    }

    #[test]
    fn test_is_binary_diff_section_allows_text() {
        let section = "diff --git a/src/main.rs b/src/main.rs\nindex abc1234..def5678 100644\n--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,1 +1,2 @@\n fn main() {}\n+fn added() {}\n";
        assert!(!is_binary_diff_section(section));
    }
}
