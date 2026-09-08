use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::working_log::{AgentId, Checkpoint, CheckpointKind, WorkingLogEntry};
use git_ai::operations::commands::checkpoint_agent::orchestrator::{
    BaseCommit, CheckpointFile, CheckpointRequest,
};
use git_ai::operations::daemon::checkpoint::{
    PreparedPathRole, ResolvedCheckpointExecution, compute_file_line_stats,
    execute_resolved_checkpoint_from_daemon, is_ai_author_id,
};
use git_ai::operations::git::repository::find_repository_in_path;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// Helper function equivalent to TmpRepo::new_with_base_commit()
fn setup_repo_with_base_commit() -> (TestRepo, String, String) {
    let repo = TestRepo::new();

    let lines_content = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20\n21\n22\n23\n24\n25\n26\n";
    let alphabet_content =
        "A\nB\nC\nD\nE\nF\nG\nH\nI\nJ\nK\nL\nM\nN\nO\nP\nQ\nR\nS\nT\nU\nV\nW\nX\nY\nZ\n";

    std::fs::write(repo.path().join("lines.md"), lines_content).unwrap();
    std::fs::write(repo.path().join("alphabet.md"), alphabet_content).unwrap();
    repo.git(&["add", "lines.md", "alphabet.md"]).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "lines.md"])
        .unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "alphabet.md"])
        .unwrap();
    repo.stage_all_and_commit("initial commit").unwrap();

    (repo, "lines.md".to_string(), "alphabet.md".to_string())
}

mod attribution;
mod line_statistics;
mod path_filtering;
mod staging;
mod storage;
