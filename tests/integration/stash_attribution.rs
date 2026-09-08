use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::attribution_tracker::LineAttribution;
use git_ai::model::authorship_log::{HumanRecord, PromptRecord, SessionRecord};
use git_ai::model::working_log::AgentId;
use git_ai::operations::git::repo_storage::InitialAttributions;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::PathBuf;

fn stash_v2_dir(repo: &TestRepo) -> PathBuf {
    repo.path().join(".git").join("ai").join("stashes_v2")
}

fn single_stash_v2_initial(repo: &TestRepo) -> InitialAttributions {
    let stashes = stash_v2_dir(repo);
    let stash_dir = fs::read_dir(&stashes)
        .expect("stashes_v2 dir exists")
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("a compact stash dir should exist");
    let initial = fs::read_to_string(stash_dir.join("INITIAL")).expect("stash INITIAL exists");
    serde_json::from_str(&initial).expect("stash INITIAL is valid")
}

fn current_checkpoint_files(repo: &TestRepo) -> BTreeSet<String> {
    repo.current_working_logs()
        .read_all_checkpoints()
        .expect("read current checkpoints")
        .into_iter()
        .flat_map(|checkpoint| checkpoint.entries.into_iter().map(|entry| entry.file))
        .collect()
}

fn joke_lines(file_idx: usize, count: usize) -> Vec<String> {
    (0..count)
        .map(|line_idx| format!("joke file {file_idx} line {line_idx}: boilerplate punchline"))
        .collect()
}

fn lines_to_content(lines: &[String]) -> String {
    let mut content = lines.join("\n");
    content.push('\n');
    content
}

fn test_agent(id: &str) -> AgentId {
    AgentId {
        tool: "test".to_string(),
        id: id.to_string(),
        model: "test-model".to_string(),
    }
}

fn test_prompt(id: &str) -> PromptRecord {
    PromptRecord {
        agent_id: test_agent(id),
        human_author: None,
        messages_url: None,
        total_additions: 0,
        total_deletions: 0,
        accepted_lines: 0,
        overriden_lines: 0,
        custom_attributes: None,
    }
}

mod checkpoint_storage;
mod conflict_resolution;
mod cross_branch_recovery;
mod line_attribution;
mod pathspecs;
mod stack_operations;
