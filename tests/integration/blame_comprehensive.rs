//! Comprehensive tests for src/commands/blame.rs
//!
//! This test module covers critical functionality in blame.rs (1,811 LOC)
//! including integration tests for AI authorship overlay, error handling,
//! edge cases, and output formatting.
//!
//! Test coverage areas:
//! 1. Core blame functionality with AI authorship
//! 2. Error handling (invalid refs, missing files, git errors)
//! 3. Edge cases (empty files, binary files, renamed files)
//! 4. Output formatting (default, porcelain, incremental, JSON)
//! 5. Line range handling
//! 6. Commit filtering (newest_commit, oldest_commit, oldest_date)
//! 7. AI authorship splitting by human author
//! 8. Foreign prompt lookups
//! 9. File path normalization (absolute vs relative)

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;

use git_ai::model::authorship_log::{LineRange, PromptRecord};
use git_ai::model::authorship_log_serialization::{
    AttestationEntry, AuthorshipLog, FileAttestation,
};
use git_ai::model::working_log::AgentId;
use git_ai::operations::commands::blame::GitAiBlameOptions;
use git_ai::operations::git::notes_api::write_note;
use git_ai::operations::git::repository as GitAiRepository;

mod file_content;
mod formatting;
mod input_validation;
mod line_attribution;
mod ranges_and_analysis;
