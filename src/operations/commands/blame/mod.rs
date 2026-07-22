use chrono::{DateTime, FixedOffset, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::LazyLock;

use crate::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};

mod args;
mod computation;
mod hunk_parser;
mod json_output;
mod output;
mod overlay;
mod porcelain;

pub use args::parse_blame_args;

//🐰🥚 @todo use actual date Git AI was installed in each repo
pub static OLDEST_AI_BLAME_DATE: LazyLock<DateTime<FixedOffset>> = LazyLock::new(|| {
    FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2025, 7, 4, 0, 0, 0)
        .unwrap()
});

#[derive(Debug, Clone, Serialize)]
pub struct BlameHunk {
    /// Line range [start, end] (inclusive) - current line numbers in the file
    pub range: (u32, u32),
    /// Original line range [start, end] (inclusive) - line numbers in the commit that introduced them
    pub orig_range: (u32, u32),
    /// Commit SHA that introduced this hunk
    pub commit_sha: String,
    /// Abbreviated commit SHA
    #[allow(dead_code)]
    pub abbrev_sha: String,
    /// Original author from Git blame
    pub original_author: String,
    /// Author email
    pub author_email: String,
    /// Author time (unix timestamp)
    pub author_time: i64,
    /// Author timezone (e.g. "+0000")
    pub author_tz: String,
    /// AI human author name
    pub ai_human_author: Option<String>,
    /// Committer name
    pub committer: String,
    /// Committer email
    pub committer_email: String,
    /// Committer time (unix timestamp)
    pub committer_time: i64,
    /// Committer timezone
    pub committer_tz: String,
    /// Whether this is a boundary commit
    pub is_boundary: bool,
    /// The filename at the blamed commit (may differ from current if file was renamed)
    #[serde(default)]
    pub orig_filename: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BlameAnalysisResult {
    pub line_authors: HashMap<u32, String>,
    pub prompt_records: HashMap<String, PromptRecord>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub session_records: HashMap<String, SessionRecord>,
    pub blame_hunks: Vec<BlameHunk>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub humans: BTreeMap<String, HumanRecord>,
}

pub(super) struct PreparedBlameRequest {
    pub relative_file_path: String,
    pub file_content: String,
    pub line_ranges: Vec<(u32, u32)>,
    pub options: GitAiBlameOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GitAiBlameOptions {
    // Line range options
    pub line_ranges: Vec<(u32, u32)>,

    pub newest_commit: Option<String>,
    pub oldest_commit: Option<String>,
    pub oldest_date: Option<DateTime<FixedOffset>>,
    /// Raw git --since value (e.g. "2 weeks ago", "2026-01-01"), used when callers
    /// need idiomatic git date parsing without pre-parsing to RFC3339.
    pub oldest_date_spec: Option<String>,

    // Output format options
    pub porcelain: bool,
    pub line_porcelain: bool,
    pub incremental: bool,
    pub show_name: bool,
    pub show_number: bool,
    pub show_email: bool,
    pub suppress_author: bool,
    pub show_stats: bool,

    // Commit display options
    pub long_rev: bool,
    pub raw_timestamp: bool,
    pub abbrev: Option<u32>,

    // Boundary options
    pub blank_boundary: bool,
    pub show_root: bool,

    // Movement detection options
    pub detect_moves: bool,
    pub detect_copies: u32, // Number of -C flags (0-3)
    pub move_threshold: Option<u32>,

    // Ignore options
    pub ignore_revs: Vec<String>,
    pub ignore_revs_file: Option<String>,
    /// Disable auto-detection of .git-blame-ignore-revs file
    pub no_ignore_revs_file: bool,

    // Color options
    pub color_lines: bool,
    pub color_by_age: bool,

    // Progress options
    pub progress: bool,

    // Date format
    pub date_format: Option<String>,

    // Content options
    pub contents_file: Option<String>,

    // Revision options
    #[allow(dead_code)]
    pub reverse: Option<String>,
    pub first_parent: bool,

    // Encoding
    pub encoding: Option<String>,

    // Pre-read contents data (from --contents flag, either from stdin or file)
    // This is populated during argument parsing and used by blame
    pub contents_data: Option<Vec<u8>>,

    // Use prompt hashes as name instead of author names
    pub use_prompt_hashes_as_names: bool,

    // Return all human authors as CheckpointKind::Human
    pub return_human_authors_as_human: bool,

    // No output
    pub no_output: bool,

    // Ignore whitespace
    pub ignore_whitespace: bool,

    // JSON output format
    pub json: bool,

    // Mark lines from commits without authorship logs as "Unknown"
    pub mark_unknown: bool,

    // Show prompt hashes inline and dump prompts when piped
    pub show_prompt: bool,

    // Split hunks when lines have different AI human authors
    // When true, a single git blame hunk may be split into multiple hunks
    // if different lines were authored by different humans working with AI
    pub split_hunks_by_ai_author: bool,
}

impl Default for GitAiBlameOptions {
    fn default() -> Self {
        Self {
            line_ranges: Vec::new(),
            porcelain: false,
            newest_commit: None,
            oldest_commit: None,
            oldest_date: None,
            oldest_date_spec: None,
            line_porcelain: false,
            incremental: false,
            show_name: false,
            show_number: false,
            show_email: false,
            suppress_author: false,
            show_stats: false,
            long_rev: false,
            raw_timestamp: false,
            abbrev: None,
            blank_boundary: false,
            show_root: false,
            detect_moves: false,
            detect_copies: 0,
            move_threshold: None,
            ignore_revs: Vec::new(),
            ignore_revs_file: None,
            no_ignore_revs_file: false,
            color_lines: false,
            color_by_age: false,
            progress: false,
            date_format: None,
            contents_file: None,
            reverse: None,
            first_parent: false,
            encoding: None,
            contents_data: None,
            use_prompt_hashes_as_names: false,
            return_human_authors_as_human: false,
            no_output: false,
            ignore_whitespace: false,
            json: false,
            mark_unknown: false,
            show_prompt: false,
            split_hunks_by_ai_author: true,
        }
    }
}
