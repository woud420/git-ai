//! Comprehensive tests for src/git/repository.rs
//!
//! This test suite covers the core git operations layer including:
//! - Repository initialization and discovery
//! - Git command execution and error handling
//! - HEAD operations and branch management
//! - Commit operations and traversal
//! - Config get/set operations
//! - Pathspec validation and filtering
//! - Rewrite log operations
//! - Error handling and edge cases
//! - Working directory operations
//! - Bare repository support

use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::operations::git::repository::{find_repository, find_repository_in_path};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

mod commit_objects;
mod configuration_and_remotes;
mod discovery;
mod history_and_file_queries;
mod tree_and_blob_objects;
