use super::tests::{family_state, normalized};
use super::*;
use crate::model::domain::RefChange;

#[test]
fn reset_identity_preserves_worktree_branch_state() {
    let mut state = family_state();
    let path = PathBuf::from("/tmp/repo");
    state.worktrees.insert(
        path.clone(),
        WorktreeState {
            head: Some("aaa".to_string()),
            branch: Some("refs/heads/main".to_string()),
            detached: false,
            last_updated_ns: 1,
        },
    );
    let mut cmd = normalized();
    cmd.primary_command = Some("reset".to_string());
    cmd.invoked_command = Some("reset".to_string());
    cmd.invoked_args = vec!["--hard".to_string(), "HEAD".to_string()];
    cmd.ref_changes = vec![RefChange {
        reference: "HEAD".to_string(),
        old: "aaa".to_string(),
        new: "aaa".to_string(),
    }];
    reduce_family_command(&mut state, cmd, &AnalyzerRegistry::new()).unwrap();
    let worktree = &state.worktrees[&path];
    assert_eq!(worktree.head.as_deref(), Some("aaa"));
    assert_eq!(worktree.branch.as_deref(), Some("refs/heads/main"));
    assert!(!worktree.detached);
}
