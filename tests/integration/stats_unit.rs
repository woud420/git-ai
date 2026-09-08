use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log::LineRange;
use git_ai::model::authorship_log::PromptRecord;
use git_ai::model::authorship_log_serialization::AttestationEntry;
use git_ai::model::authorship_log_serialization::AuthorshipLog;
use git_ai::model::authorship_log_serialization::FileAttestation;
use git_ai::model::authorship_log_serialization::generate_short_hash;
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::repository::find_repository_in_path;
use std::collections::BTreeMap;
use std::collections::HashMap;

mod accepted_lines;
mod commit_statistics;
mod ignore_patterns;
