use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::clients::git_cli::{exec_git, exec_git_stdin};
use crate::error::GitAiError;
use crate::operations::git::repository::Repository;

use super::{BlameHunk, GitAiBlameOptions};

pub(super) struct BlameRenderPreparation {
    pub(super) hunks: Vec<BlameHunk>,
    pub(super) line_to_hunk: HashMap<u32, BlameHunk>,
    pub(super) requested_lines: Vec<u32>,
}

fn prepare_blame_hunks(hunks: Vec<BlameHunk>) -> BlameRenderPreparation {
    let mut line_to_hunk = HashMap::new();
    for hunk in &hunks {
        for line_num in hunk.range.0..=hunk.range.1 {
            line_to_hunk.insert(line_num, hunk.clone());
        }
    }
    let mut requested_lines = line_to_hunk.keys().copied().collect::<Vec<_>>();
    requested_lines.sort_unstable();
    BlameRenderPreparation {
        hunks,
        line_to_hunk,
        requested_lines,
    }
}

impl Repository {
    pub fn blame_hunks(
        &self,
        file_path: &str,
        start_line: u32,
        end_line: u32,
        options: &GitAiBlameOptions,
    ) -> Result<Vec<BlameHunk>, GitAiError> {
        self.blame_hunks_for_ranges(file_path, &[(start_line, end_line)], options)
    }

    pub fn blame_hunks_for_ranges(
        &self,
        file_path: &str,
        line_ranges: &[(u32, u32)],
        options: &GitAiBlameOptions,
    ) -> Result<Vec<BlameHunk>, GitAiError> {
        if line_ranges.is_empty() {
            return Ok(Vec::new());
        }

        // Build git blame --line-porcelain command
        let mut args = self.global_args_for_exec();
        args.push("blame".to_string());
        args.push("--line-porcelain".to_string());

        // Ignore whitespace option
        if options.ignore_whitespace {
            args.push("-w".to_string());
        }

        // Detect lines moved within a file (-M) and copied from other files (-C, implies -M).
        // Needed so that lines shifted by an adjacent insertion/deletion are traced back to the
        // commit that originally wrote them rather than the commit that moved them.
        if options.detect_moves {
            args.push("-M".to_string());
        }
        for _ in 0..options.detect_copies {
            args.push("-C".to_string());
        }

        // Respect ignore options in use
        for rev in &options.ignore_revs {
            args.push("--ignore-rev".to_string());
            args.push(rev.clone());
        }
        if let Some(file) = &options.ignore_revs_file {
            args.push("--ignore-revs-file".to_string());
            args.push(file.clone());
        }

        // Limit to the specified ranges (git blame supports multiple -L flags).
        for (start_line, end_line) in line_ranges {
            args.push("-L".to_string());
            args.push(format!("{},{}", start_line, end_line));
        }

        // Add --since flag if oldest_date is specified
        // This controls the absolute lower bound of how far back to look
        if let Some(ref date_spec) = options.oldest_date_spec {
            args.push("--since".to_string());
            args.push(date_spec.clone());
        } else if let Some(ref date) = options.oldest_date {
            args.push("--since".to_string());
            args.push(date.to_rfc3339());
        }

        // Support newest_commit option (equivalent to libgit2's newest_commit)
        // This limits blame to only consider commits up to and including the specified commit
        // When oldest_commit is also set, we use a range: oldest_commit..newest_commit
        match (&options.oldest_commit, &options.newest_commit) {
            (Some(oldest), Some(newest)) => {
                // Use range format: git blame START_COMMIT..END_COMMIT -- file.txt
                args.push(format!("{}..{}", oldest, newest));
            }
            (None, Some(newest)) => {
                // Only newest_commit set, use it as the commit to blame at
                args.push(newest.clone());
            }
            (Some(_oldest), None) => {
                // oldest_commit without newest_commit doesn't make sense for blame
                // Just ignore oldest_commit in this case
            }
            (None, None) => {
                // No commit specified, blame at HEAD (default)
            }
        }

        // Add --contents flag if we have content data to pass via stdin
        if options.contents_data.is_some() {
            args.push("--contents".to_string());
            args.push("-".to_string());
        }

        args.push("--".to_string());
        args.push(file_path.to_string());

        // Execute git blame, using stdin if we have contents data
        let output = if let Some(ref data) = options.contents_data {
            exec_git_stdin(&args, data)?
        } else {
            exec_git(&args)?
        };
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

        let mut hunks = parse_porcelain_blame_output(&stdout, file_path);

        self.populate_hunk_abbrev_shas(&mut hunks, options);

        // Post-process hunks to populate ai_human_author from authorship logs
        let hunks = self.populate_ai_human_authors(hunks, file_path, options)?;

        Ok(hunks)
    }

    pub(super) fn prepare_blame_render(
        &self,
        file_path: &str,
        line_ranges: &[(u32, u32)],
        options: &GitAiBlameOptions,
    ) -> Result<BlameRenderPreparation, GitAiError> {
        let mut no_split_options = options.clone();
        no_split_options.split_hunks_by_ai_author = false;
        self.blame_hunks_for_ranges(file_path, line_ranges, &no_split_options)
            .map(prepare_blame_hunks)
    }

    pub(super) fn blame_hunk_marker_width(hunk: &BlameHunk, options: &GitAiBlameOptions) -> usize {
        usize::from(hunk.is_boundary && !options.blank_boundary && !options.show_root)
            + usize::from(hunk.is_unblamable)
            + usize::from(hunk.is_ignored)
    }

    fn blame_marker_width(&self, hunks: &[BlameHunk], options: &GitAiBlameOptions) -> usize {
        let marker_width = hunks
            .iter()
            .map(|hunk| Self::blame_hunk_marker_width(hunk, options))
            .max()
            .unwrap_or(0);
        if marker_width == 1 {
            return 1;
        }

        // Git 2.56 reserves the largest displayed combination of ^, *, and ?.
        if self.blame_uses_dynamic_marker_width() {
            marker_width
        } else {
            1
        }
    }

    fn blame_uses_dynamic_marker_width(&self) -> bool {
        // Git's configured binary is process-global; query its version at most once.
        static USES_DYNAMIC_MARKER_WIDTH: OnceLock<bool> = OnceLock::new();
        *USES_DYNAMIC_MARKER_WIDTH.get_or_init(|| {
            self.git_version()
                .is_some_and(|version| version >= (2, 56, 0))
        })
    }

    fn resolve_blame_abbrev_width_batched(&self, hunks: &[BlameHunk]) -> usize {
        let commit_shas: HashSet<&str> = hunks
            .iter()
            .map(|hunk| hunk.commit_sha.as_str())
            // Uncommitted lines have a zero object ID, not a readable commit.
            .filter(|sha| !sha.bytes().all(|byte| byte == b'0'))
            .collect();
        if commit_shas.is_empty() {
            let Some(hunk) = hunks.first() else {
                return 7;
            };
            // rev-parse accepts a full zero OID without requiring a commit.
            // One lookup preserves core.abbrev for wholly uncommitted content.
            let mut args = self.global_args_for_exec();
            args.extend([
                "rev-parse".to_string(),
                "--short".to_string(),
                hunk.commit_sha.clone(),
            ]);
            return exec_git(&args)
                .ok()
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .and_then(|stdout| {
                    let short_sha = stdout.trim();
                    (short_sha.len() >= 4 && hunk.commit_sha.starts_with(short_sha))
                        .then_some(short_sha.len())
                })
                .unwrap_or(7);
        }

        // One stdin batch avoids argv limits and per-commit/marker spawns.
        // %h uses Git's own unique abbreviation and core.abbrev configuration.
        let mut args = self.global_args_for_exec();
        args.extend([
            "log".to_string(),
            "--no-walk=unsorted".to_string(),
            "--format=%H%x00%h".to_string(),
            "--no-show-signature".to_string(),
            "--no-notes".to_string(),
            "--no-patch".to_string(),
            "--no-decorate".to_string(),
            "--no-color".to_string(),
            "--stdin".to_string(),
        ]);
        let mut input = commit_shas.iter().copied().collect::<Vec<_>>().join("\n");
        input.push('\n');
        exec_git_stdin(&args, input.as_bytes())
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|stdout| {
                stdout
                    .lines()
                    .filter_map(|line| {
                        let (full_sha, short_sha) = line.split_once('\0')?;
                        (commit_shas.contains(full_sha)
                            && short_sha.len() >= 4
                            && full_sha.starts_with(short_sha))
                        .then_some(short_sha.len())
                    })
                    .max()
            })
            .unwrap_or(7)
    }

    fn populate_hunk_abbrev_shas(&self, hunks: &mut [BlameHunk], options: &GitAiBlameOptions) {
        if options.long_rev {
            for hunk in hunks {
                hunk.abbrev_sha = hunk.commit_sha.clone();
            }
            return;
        }

        let base_width = match options.abbrev {
            Some(0) => 40,
            Some(width) => width.clamp(4, 40) as usize,
            None => self.resolve_blame_abbrev_width_batched(hunks),
        };
        let column_width = if options.abbrev.is_none() && base_width == 40 {
            // Git treats false as GIT_MAX_HEXSZ (64), while numeric 40 stays
            // 40. Legacy Git reserves another column even for full hashes.
            let full_hashes = self
                .get_git_config_file()
                .ok()
                .and_then(|config| config.boolean("core.abbrev").and_then(Result::ok))
                == Some(false);
            let native_width = if full_hashes { 64 } else { 40 };
            native_width + usize::from(!self.blame_uses_dynamic_marker_width())
        } else {
            (base_width + self.blame_marker_width(hunks, options)).min(40)
        };
        for hunk in hunks.iter_mut() {
            let hash_width =
                column_width.saturating_sub(Self::blame_hunk_marker_width(hunk, options));
            hunk.abbrev_sha = hunk.commit_sha[..hash_width.min(hunk.commit_sha.len())].to_string();
        }
    }
}

/// Parse git blame --line-porcelain output into a list of hunks.
fn parse_porcelain_blame_output(stdout: &str, file_path: &str) -> Vec<BlameHunk> {
    // Parser state for current hunk
    #[derive(Default)]
    struct CurMeta {
        author: String,
        author_mail: String,
        author_time: i64,
        author_tz: String,
        committer: String,
        committer_mail: String,
        committer_time: i64,
        committer_tz: String,
        boundary: bool,
        ignored: bool,
        unblamable: bool,
        filename: Option<String>,
    }

    let mut hunks: Vec<BlameHunk> = Vec::new();
    let mut cur_commit: Option<String> = None;
    let mut cur_final_start: u32 = 0;
    let mut cur_orig_start: u32 = 0;
    let mut cur_group_size: u32 = 0;
    let mut cur_meta = CurMeta::default();

    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }

        if line.starts_with('\t') {
            // Content line; nothing to do, boundaries are driven by headers
            continue;
        }

        // Metadata lines
        if let Some(rest) = line.strip_prefix("author ") {
            cur_meta.author = rest.to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("author-mail ") {
            // Usually in form: <mail>
            cur_meta.author_mail = rest
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("author-time ") {
            if let Ok(t) = rest.trim().parse::<i64>() {
                cur_meta.author_time = t;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("author-tz ") {
            cur_meta.author_tz = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("committer ") {
            cur_meta.committer = rest.to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("committer-mail ") {
            cur_meta.committer_mail = rest
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("committer-time ") {
            if let Ok(t) = rest.trim().parse::<i64>() {
                cur_meta.committer_time = t;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("committer-tz ") {
            cur_meta.committer_tz = rest.trim().to_string();
            continue;
        }
        if line == "boundary" {
            cur_meta.boundary = true;
            continue;
        }
        if line == "ignored" {
            cur_meta.ignored = true;
            continue;
        }
        if line == "unblamable" {
            cur_meta.unblamable = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("filename ") {
            cur_meta.filename = Some(crate::operations::git::path_format::unescape_git_path(rest));
            continue;
        }

        // Header line: either 4 fields (new hunk) or 3 fields (continuation)
        let mut parts = line.split_whitespace();
        let sha = parts.next().unwrap_or("");
        let p2 = parts.next().unwrap_or("");
        let p3 = parts.next().unwrap_or("");
        let p4 = parts.next();

        let is_header = !sha.is_empty()
            && sha.chars().all(|c| c.is_ascii_hexdigit())
            && !p2.is_empty()
            && !p3.is_empty();
        if !is_header {
            continue;
        }

        // If we encounter a new hunk header (4 fields), flush previous hunk first
        if p4.is_some() {
            if let Some(prev_sha) = cur_commit.take() {
                // Push the previous hunk
                let start = cur_final_start;
                let end = if cur_group_size > 0 {
                    start + cur_group_size - 1
                } else {
                    start
                };
                let orig_start = cur_orig_start;
                let orig_end = if cur_group_size > 0 {
                    orig_start + cur_group_size - 1
                } else {
                    orig_start
                };

                let orig_filename = cur_meta.filename.take().filter(|f| f != file_path);
                hunks.push(BlameHunk {
                    range: (start, end),
                    orig_range: (orig_start, orig_end),
                    commit_sha: prev_sha,
                    abbrev_sha: String::new(),
                    original_author: cur_meta.author.clone(),
                    author_email: cur_meta.author_mail.clone(),
                    author_time: cur_meta.author_time,
                    author_tz: cur_meta.author_tz.clone(),
                    ai_human_author: None,
                    committer: cur_meta.committer.clone(),
                    committer_email: cur_meta.committer_mail.clone(),
                    committer_time: cur_meta.committer_time,
                    committer_tz: cur_meta.committer_tz.clone(),
                    is_boundary: cur_meta.boundary,
                    is_ignored: cur_meta.ignored,
                    is_unblamable: cur_meta.unblamable,
                    orig_filename,
                });
            }

            // Start new hunk
            cur_commit = Some(sha.to_string());
            // According to docs: fields are orig_lineno, final_lineno, group_size
            let orig_start = p2.parse::<u32>().unwrap_or(0);
            let final_start = p3.parse::<u32>().unwrap_or(0);
            let group = p4.unwrap_or("1").parse::<u32>().unwrap_or(1);
            cur_orig_start = orig_start;
            cur_final_start = final_start;
            cur_group_size = group;
            // Reset metadata for the new hunk
            cur_meta = CurMeta::default();
        } else {
            // 3-field header: continuation line within current hunk
            // Nothing to do for grouping since we use recorded group_size
            // Metadata remains from the first line of the hunk
            if cur_commit.is_none() {
                // Defensive: if no current hunk, start one with size 1
                cur_commit = Some(sha.to_string());
                cur_orig_start = p2.parse::<u32>().unwrap_or(0);
                cur_final_start = p3.parse::<u32>().unwrap_or(0);
                cur_group_size = 1;
            }
        }
    }

    // Flush the final hunk if present
    if let Some(prev_sha) = cur_commit.take() {
        let start = cur_final_start;
        let end = if cur_group_size > 0 {
            start + cur_group_size - 1
        } else {
            start
        };
        let orig_start = cur_orig_start;
        let orig_end = if cur_group_size > 0 {
            orig_start + cur_group_size - 1
        } else {
            orig_start
        };

        let orig_filename = cur_meta.filename.take().filter(|f| f != file_path);
        hunks.push(BlameHunk {
            range: (start, end),
            orig_range: (orig_start, orig_end),
            commit_sha: prev_sha,
            abbrev_sha: String::new(),
            original_author: cur_meta.author.clone(),
            author_email: cur_meta.author_mail.clone(),
            author_time: cur_meta.author_time,
            author_tz: cur_meta.author_tz.clone(),
            ai_human_author: None,
            committer: cur_meta.committer.clone(),
            committer_email: cur_meta.committer_mail.clone(),
            committer_time: cur_meta.committer_time,
            committer_tz: cur_meta.committer_tz.clone(),
            is_boundary: cur_meta.boundary,
            is_ignored: cur_meta.ignored,
            is_unblamable: cur_meta.unblamable,
            orig_filename,
        });
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_preparation_handles_empty_overlap_and_owned_sorting() {
        let empty = prepare_blame_hunks(vec![]);
        assert!(empty.hunks.is_empty() && empty.line_to_hunk.is_empty());
        assert!(empty.requested_lines.is_empty());
        let hunks = parse_porcelain_blame_output("a 2 2 2\nb 1 1 2", "f");
        let mut prepared = prepare_blame_hunks(hunks);
        assert_eq!(prepared.requested_lines, [1, 2, 3]);
        assert_eq!(prepared.hunks[1].commit_sha, "b");
        assert_eq!(prepared.line_to_hunk[&2].commit_sha, "b");
        prepared.hunks[1].commit_sha = "changed".to_string();
        assert_eq!(prepared.line_to_hunk[&2].commit_sha, "b");
    }
}
