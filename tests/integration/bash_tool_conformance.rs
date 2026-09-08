//! Conformance test suite for the bash tool change attribution feature.
//!
//! Covers PRD Sections 5.1 (file mutations), 5.2 (read-only operations),
//! 5.3 (edge cases), 5.4 (pre/post hook semantics), tool classification
//! for all six agents, gitignore filtering, and full handle_bash_tool
//! orchestration.

use crate::bash_tool_common::{add_and_commit, post_hook, pre_hook, repo_root};
use crate::repos::test_repo::TestRepo;
use git_ai::operations::commands::checkpoint_agent::bash_tool::{
    Agent, BashCheckpointAction, StatDiffResult, StatEntry, StatFileType, StatSnapshot, ToolClass,
    build_gitignore, classify_tool, diff, git_status_fallback, normalize_path, snapshot,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

mod file_changes;
mod hook_orchestration;
mod ignored_paths;
mod stat_snapshot;
mod status_fallback;
mod tool_classification;
