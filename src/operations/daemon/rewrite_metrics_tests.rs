use super::*;
use crate::metrics::EventValues;
use crate::metrics::events::rewrite_committed_pos;
use crate::model::authorship_log::LineRange;
use crate::model::authorship_log_serialization::AttestationEntry;
use crate::model::working_log::AgentId;
use crate::operations::authorship::rewrite::RewriteMetricOperation;
use std::collections::HashMap;

fn metric_commit(
    new_sha: &str,
    originals: &[&str],
    operation: RewriteMetricOperation,
) -> RewriteMetricCommit {
    RewriteMetricCommit::new(
        new_sha.to_string(),
        originals.iter().map(|s| s.to_string()).collect(),
        operation,
    )
}

fn note_for_ai_line(file_path: &str, line: u32) -> String {
    let prompt_id = "prompt1".to_string();
    let mut log = AuthorshipLog::new();
    log.metadata.prompts.insert(
        prompt_id.clone(),
        crate::model::authorship_log::PromptRecord {
            agent_id: AgentId {
                tool: "codex".to_string(),
                id: "session".to_string(),
                model: "gpt-5".to_string(),
            },
            human_author: None,
            messages_url: None,
            total_additions: 0,
            total_deletions: 0,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: None,
        },
    );
    log.get_or_create_file(file_path)
        .add_entry(AttestationEntry::new(
            prompt_id,
            vec![LineRange::Single(line)],
        ));
    log.serialize_to_string().expect("serialize note")
}

#[test]
fn dedupe_metric_commits_keeps_distinct_original_sets() {
    let first = metric_commit("new", &["old1"], RewriteMetricOperation::Rebase);
    let second = metric_commit("new", &["old1"], RewriteMetricOperation::Rebase);
    let squash = metric_commit(
        "new",
        &["old1", "old2"],
        RewriteMetricOperation::SquashMerge,
    );

    let result = dedupe_metric_commits(vec![first.clone(), second, squash.clone()]);

    assert_eq!(result, vec![first, squash]);
}

#[test]
fn rewrite_event_schema_does_not_emit_position_10() {
    let values = RewriteCommittedValues::new()
        .human_additions(1)
        .git_diff_deleted_lines(2)
        .git_diff_added_lines(3)
        .tool_model_pairs(vec!["all".to_string()])
        .ai_additions(vec![1])
        .ai_accepted(vec![1])
        .commit_subject("subject")
        .commit_body_null()
        .authorship_note("note")
        .hunks("[]")
        .operation_kind("rebase")
        .original_commit_shas(vec!["old".to_string()]);

    let sparse = PosEncoded::to_sparse(&values);

    assert_eq!(
        RewriteCommittedValues::event_id(),
        crate::metrics::types::MetricEventId::RewriteCommitted
    );
    assert!(!sparse.contains_key("10"));
    assert_eq!(
        sparse.get(&rewrite_committed_pos::OPERATION_KIND.to_string()),
        Some(&serde_json::json!("rebase"))
    );
    assert_eq!(
        sparse.get(&rewrite_committed_pos::ORIGINAL_COMMIT_SHAS.to_string()),
        Some(&serde_json::json!(["old"]))
    );
}

#[test]
fn rewrite_metric_branch_overrides_head_branch_attr() {
    let commit = metric_commit("new", &["old"], RewriteMetricOperation::NonFastForward)
        .with_branch("feature");
    let attrs = apply_rewrite_metric_branch(
        crate::metrics::EventAttributes::with_version("test").branch("main"),
        &commit,
    );
    let sparse = attrs.to_sparse();

    assert_eq!(
        sparse.get(&crate::metrics::attrs::attr_pos::BRANCH.to_string()),
        Some(&serde_json::json!("feature"))
    );
}

#[test]
fn rewrite_metric_custom_attributes_match_map_builder_wire_format() {
    let mut custom_attributes = HashMap::new();
    custom_attributes.insert("team".to_string(), "metrics".to_string());
    let custom_attributes_json =
        serde_json::to_string(&custom_attributes).expect("serialize custom attributes");

    let sparse_from_batch_json = apply_rewrite_metric_custom_attributes(
        crate::metrics::EventAttributes::with_version("test"),
        Some(&custom_attributes_json),
    )
    .to_sparse();
    let sparse_from_map = crate::metrics::EventAttributes::with_version("test")
        .custom_attributes_map(&custom_attributes)
        .to_sparse();

    assert_eq!(
        sparse_from_batch_json.get(&crate::metrics::attrs::attr_pos::CUSTOM_ATTRIBUTES.to_string()),
        sparse_from_map.get(&crate::metrics::attrs::attr_pos::CUSTOM_ATTRIBUTES.to_string())
    );
}

#[test]
fn rewrite_metric_event_uses_supplied_note_and_parent_diff() {
    let tmp = crate::operations::git::test_utils::TmpRepo::new().expect("tmp repo");
    let note = note_for_ai_line("file.txt", 1);

    let mut hunks_by_file = HashMap::new();
    hunks_by_file.insert(
        "file.txt".to_string(),
        vec![crate::model::hunk_shift::DiffHunk {
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
        }],
    );
    let parent_diff = DiffTreeResult {
        hunks_by_file,
        added_lines_by_file: HashMap::new(),
        renames: Vec::new(),
    };
    let commit = metric_commit("new", &["old"], RewriteMetricOperation::Rebase)
        .with_branch("feature")
        .with_parent_sha("parent")
        .with_authorship_note(note.clone())
        .with_parent_diff(parent_diff);

    let batch_context = RewriteMetricBatchContext::new(tmp.gitai_repo());
    let event = build_rewrite_committed_metric_event(&commit, &batch_context)
        .expect("metric build")
        .expect("event");

    assert_eq!(
        event
            .values
            .get(&rewrite_committed_pos::AUTHORSHIP_NOTE.to_string()),
        Some(&serde_json::json!(note))
    );
    assert_eq!(
        event
            .values
            .get(&rewrite_committed_pos::GIT_DIFF_ADDED_LINES.to_string()),
        Some(&serde_json::json!(1))
    );
    assert_eq!(
        event
            .values
            .get(&rewrite_committed_pos::TOOL_MODEL_PAIRS.to_string()),
        Some(&serde_json::json!(["all", "codex::gpt-5"]))
    );
    assert_eq!(
        event
            .attrs
            .get(&crate::metrics::attrs::attr_pos::BRANCH.to_string()),
        Some(&serde_json::json!("feature"))
    );
    assert_eq!(
        event
            .attrs
            .get(&crate::metrics::attrs::attr_pos::BASE_COMMIT_SHA.to_string()),
        Some(&serde_json::json!("parent"))
    );
}

#[test]
fn rewrite_metric_worker_hydrates_missing_parent_and_diff() {
    let tmp = crate::operations::git::test_utils::TmpRepo::new().expect("tmp repo");
    tmp.write_file("file.txt", "base\n", false)
        .expect("write base");
    let parent_sha = tmp.commit_all("base").expect("base commit");
    tmp.write_file("file.txt", "base\nai\n", false)
        .expect("write update");
    let new_sha = tmp.commit_all("update").expect("update commit");
    let note = note_for_ai_line("file.txt", 2);

    let commit = metric_commit(
        &new_sha,
        &[&parent_sha],
        RewriteMetricOperation::NonFastForward,
    )
    .with_authorship_note(note);

    let events = build_rewrite_metric_events(tmp.gitai_repo(), &[commit]);

    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]
            .attrs
            .get(&crate::metrics::attrs::attr_pos::BASE_COMMIT_SHA.to_string()),
        Some(&serde_json::json!(parent_sha))
    );
    assert_eq!(
        events[0]
            .values
            .get(&rewrite_committed_pos::GIT_DIFF_ADDED_LINES.to_string()),
        Some(&serde_json::json!(1))
    );
}

#[test]
fn rewrite_metric_worker_hydrates_initial_parent_diff() {
    let tmp = crate::operations::git::test_utils::TmpRepo::new().expect("tmp repo");
    tmp.write_file("file.txt", "ai\n", false)
        .expect("write root");
    let root_sha = tmp.commit_all("root").expect("root commit");
    let note = note_for_ai_line("file.txt", 1);

    let commit = metric_commit(&root_sha, &["old"], RewriteMetricOperation::Amend)
        .with_authorship_note(note);

    let events = build_rewrite_metric_events(tmp.gitai_repo(), &[commit]);

    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0]
            .attrs
            .get(&crate::metrics::attrs::attr_pos::BASE_COMMIT_SHA.to_string()),
        Some(&serde_json::json!("initial"))
    );
    assert_eq!(
        events[0]
            .values
            .get(&rewrite_committed_pos::GIT_DIFF_ADDED_LINES.to_string()),
        Some(&serde_json::json!(1))
    );
}

#[test]
fn rewrite_stats_cost_preserves_each_boundary_and_ignored_files() {
    use crate::operations::authorship::post_commit::{
        STATS_SKIP_MAX_ADDED_LINES, STATS_SKIP_MAX_DELETED_LINES,
        STATS_SKIP_MAX_FILES_WITH_ADDITIONS, STATS_SKIP_MAX_HUNKS,
    };
    use crate::operations::commands::diff::DiffHunk;

    fn hunk(path: String, added: usize, deleted: usize) -> DiffHunk {
        DiffHunk {
            file_path: path,
            old_file_path: None,
            old_start: 1,
            old_count: deleted as u32,
            new_start: 1,
            new_count: added as u32,
            deleted_lines: vec![1; deleted],
            added_lines: vec![1; added],
            deleted_contents: Vec::new(),
            added_contents: Vec::new(),
        }
    }

    for (boundary, distinct_files, added, deleted) in [
        (STATS_SKIP_MAX_HUNKS, false, 1, 0),
        (STATS_SKIP_MAX_FILES_WITH_ADDITIONS, true, 1, 0),
        (STATS_SKIP_MAX_ADDED_LINES, false, 0, 0),
        (STATS_SKIP_MAX_DELETED_LINES, false, 0, 1),
    ] {
        for count in [boundary - 1, boundary] {
            let hunks = if added == 1 {
                (0..count)
                    .map(|index| {
                        hunk(
                            format!("{}.rs", if distinct_files { index } else { 0 }),
                            1,
                            0,
                        )
                    })
                    .collect::<Vec<_>>()
            } else {
                vec![hunk(
                    "0.rs".to_string(),
                    if deleted == 0 { count } else { 0 },
                    if deleted == 1 { count } else { 0 },
                )]
            };
            assert_eq!(
                should_skip_rewrite_metric_stats(&hunks, &[]),
                count == boundary,
                "boundary={boundary}, count={count}, distinct_files={distinct_files}, added={added}, deleted={deleted}"
            );
            assert!(!should_skip_rewrite_metric_stats(
                &hunks,
                &["*.rs".to_string()]
            ));
        }
    }
}
