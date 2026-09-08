//! Comprehensive tests for `git-ai diff` command (additional coverage)
//!
//! These tests complement the existing tests/diff.rs with additional edge cases
//! and scenarios to push coverage toward 95%.

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use serde_json::Value;

mod output_contracts;
mod ranges_and_files;
