use crate::repos::test_repo::TestRepo;
use git_ai::operations::authorship::range_authorship::{
    EMPTY_TREE_HASH, range_authorship, should_ignore_file,
};
use git_ai::operations::git::repository::{CommitRange, find_repository_in_path};

mod ignore_patterns;
mod line_attribution;
mod lockfiles_and_globs;
