use super::{
    ExpectedLineExt, TestRepo, assert_edge_recovery_attribution, attr_pos,
    edge_recovery_metric_fixture, fs, sparse_str, wait_for_edge_recovery_metric,
};

#[test]
fn test_edge_extension_recovers_unknown_gap_between_ai_attributions() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("edge.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(&file_path, "base\nai before\nunknown gap\nai after\n").unwrap();
    repo.git_ai(&["checkpoint", "human", "edge.txt"])
        .expect("legacy human checkpoint should mark current content untracked");

    fs::write(
        &file_path,
        "base\nai before edited\nunknown gap\nai after edited\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "edge.txt"])
        .expect("AI checkpoint should only directly attribute the edited lines");

    repo.stage_all_and_commit("Recover edge attribution")
        .unwrap();

    let mut file = repo.filename("edge.txt");
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "ai before edited".ai(),
        "unknown gap".ai(),
        "ai after edited".ai(),
    ]);
}

#[test]
fn test_edge_extension_recovers_leading_and_trailing_unknown_lines_near_ai_block() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("edge-fringes.txt");

    fs::write(&file_path, "base\n").unwrap();
    repo.stage_all_and_commit("Initial commit").unwrap();

    fs::write(
        &file_path,
        "\
base
leading dirty
ai one placeholder
ai two placeholder
ai three placeholder
trailing dirty 1
trailing dirty 2
trailing dirty 3
trailing dirty 4
",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "human", "edge-fringes.txt"])
        .expect("legacy human checkpoint should mark current content untracked");

    fs::write(
        &file_path,
        "\
base
leading dirty
ai one
ai two
ai three
trailing dirty 1
trailing dirty 2
trailing dirty 3
trailing dirty 4
",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "edge-fringes.txt"])
        .expect("AI checkpoint should only directly attribute the edited block");

    repo.stage_all_and_commit("Recover edge fringes").unwrap();

    let mut file = repo.filename("edge-fringes.txt");
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "leading dirty".ai(),
        "ai one".ai(),
        "ai two".ai(),
        "ai three".ai(),
        "trailing dirty 1".ai(),
        "trailing dirty 2".ai(),
        "trailing dirty 3".ai(),
        "trailing dirty 4".unattributed_human(),
    ]);
}

// Regression coverage for ENG-315 and upstream git-ai-project/git-ai#2214.
#[test]
fn test_edge_extension_recovery_metric_preserves_known_identity() {
    const FILE_NAME: &str = "known-edge.txt";
    const SESSION_ID: &str = "known-edge-session";

    let fixture = edge_recovery_metric_fixture(FILE_NAME, SESSION_ID);
    fixture
        .repo
        .stage_all_and_commit("Recover edge attribution")
        .unwrap();
    assert_edge_recovery_attribution(&fixture.repo, FILE_NAME);

    let event = wait_for_edge_recovery_metric(&fixture.metrics_db_path, FILE_NAME);
    assert_eq!(sparse_str(&event.attrs, attr_pos::TOOL), Some("claude"));
    assert_eq!(
        sparse_str(&event.attrs, attr_pos::MODEL),
        Some("claude-sonnet-4")
    );
    assert_eq!(
        sparse_str(&event.attrs, attr_pos::EXTERNAL_SESSION_ID),
        Some(SESSION_ID)
    );
}

#[test]
fn test_edge_extension_recovery_metric_keeps_unknown_identity_sparse() {
    const FILE_NAME: &str = "unknown-edge.txt";
    const SESSION_ID: &str = "unknown-edge-session";

    let fixture = edge_recovery_metric_fixture(FILE_NAME, SESSION_ID);

    let working_log = fixture.repo.current_working_logs();
    let mut cleared_agent_ids = 0;
    let checkpoints = working_log
        .mutate_all_checkpoints(|checkpoints| {
            for checkpoint in checkpoints {
                if checkpoint
                    .agent_id
                    .as_ref()
                    .is_some_and(|agent_id| agent_id.id == SESSION_ID)
                {
                    checkpoint.agent_id = None;
                    cleared_agent_ids += 1;
                }
            }
            Ok(())
        })
        .unwrap();
    assert!(
        cleared_agent_ids > 0,
        "fixture should remove the unknown session's existing identity"
    );
    assert!(checkpoints.iter().all(|checkpoint| {
        checkpoint
            .agent_id
            .as_ref()
            .is_none_or(|agent_id| agent_id.id != SESSION_ID)
    }));

    fixture
        .repo
        .stage_all_and_commit("Recover edge attribution")
        .unwrap();
    let mut file = fixture.repo.filename(FILE_NAME);
    file.assert_committed_lines(lines![
        "base".unattributed_human(),
        "ai before edited".unattributed_human(),
        "unknown gap".unattributed_human(),
        "ai after edited".unattributed_human(),
    ]);

    let unknown_event = wait_for_edge_recovery_metric(&fixture.metrics_db_path, FILE_NAME);
    for pos in [
        attr_pos::TOOL,
        attr_pos::MODEL,
        attr_pos::EXTERNAL_SESSION_ID,
    ] {
        assert!(
            !unknown_event.attrs.contains_key(&pos.to_string()),
            "unknown identity field at position {pos} should stay sparse"
        );
    }
}
