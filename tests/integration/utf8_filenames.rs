use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
/// Tests for UTF-8 filename handling with Chinese characters and emojis.
///
/// This tests verifies that files with non-ASCII characters in their filenames
/// are correctly tracked and attributed when git-ai processes commits.
///
/// Issue: Files with Chinese (or other non-ASCII) characters in filenames were
/// incorrectly classified as human-written because git outputs such filenames
/// with octal escape sequences (e.g., `"\344\270\255\346\226\207.txt"` for "中文.txt").
use crate::test_utils::extract_json_object;
use git_ai::operations::authorship::stats::CommitStats;

mod cjk_scripts;
mod cyrillic_and_greek;
mod emoji_sequences;
mod indic_scripts;
mod normalization;
mod path_attribution;
mod right_to_left;
mod southeast_asian_scripts;
mod symbols;
