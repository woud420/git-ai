use super::*;
use crate::repos::test_file::ExpectedLineExt;

fn mixed_history() -> (TestRepo, String) {
    let repo = TestRepo::new();
    let mut content = String::from("base\n");
    let mut expected = vec!["base".human()];
    std::fs::write(repo.path().join("history.txt"), &content).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human", "history.txt"])
        .unwrap();
    let base = repo.stage_all_and_commit("base").unwrap().commit_sha;
    repo.filename("history.txt")
        .assert_committed_lines(expected.clone());
    for index in 1..=12 {
        let text = format!("line {index}");
        content.push_str(&text);
        content.push('\n');
        std::fs::write(repo.path().join("history.txt"), &content).unwrap();
        let preset = if index % 2 == 1 {
            "mock_ai"
        } else {
            "mock_known_human"
        };
        repo.git_ai(&["checkpoint", preset, "history.txt"]).unwrap();
        expected.push(if index % 2 == 1 {
            text.ai()
        } else {
            text.human()
        });
        repo.stage_all_and_commit(&format!("add line {index}"))
            .unwrap();
        repo.filename("history.txt")
            .assert_committed_lines(expected.clone());
    }
    (repo, base)
}

fn assert_note_read_budget(log: &std::path::Path, individual_budget: usize, batch_budget: usize) {
    let commands = std::fs::read_to_string(log).unwrap();
    // Concurrent probes can interleave their newline writes; count command tokens.
    assert!(
        commands.contains("blame"),
        "probe must observe real blame execution"
    );
    let individual = commands.matches("notes").count();
    let batches = commands.matches("cat-file").count();
    assert!(
        individual <= individual_budget,
        "individual notes reads: {individual}, budget: {individual_budget}"
    );
    assert!(
        batches > 0 && batches <= batch_budget,
        "batch reads: {batches}, budget: {batch_budget}"
    );
}

#[test]
fn blame_batches_note_reads_across_commit_history() {
    let (repo, _) = mixed_history();
    let log = repo.path().join("blame-spawns.log");
    let output = repo
        .git_ai_with_env(
            &["blame", "history.txt"],
            &[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())],
        )
        .unwrap();
    assert!(output.contains("line 12"));
    assert_note_read_budget(&log, 0, 3);
}

#[test]
fn range_stats_batches_note_reads_without_changing_attribution_totals() {
    let (repo, base) = mixed_history();
    let log = repo.path().join("stats-spawns.log");
    let range = format!("{base}..HEAD");
    let output = repo
        .git_ai_with_env(
            &["stats", &range, "--json"],
            &[("GIT_AI_SPAWN_LOG", log.to_str().unwrap())],
        )
        .unwrap();
    let json = crate::test_utils::extract_json_object(&output);
    let stats: git_ai::operations::authorship::range_authorship::RangeAuthorshipStats =
        serde_json::from_str(&json).unwrap();
    assert_eq!(stats.authorship_stats.total_commits, 12);
    assert_eq!(stats.range_stats.git_diff_added_lines, 12);
    assert_eq!(stats.range_stats.ai_additions, 6);
    assert_eq!(stats.range_stats.human_additions, 6);
    assert_eq!(stats.range_stats.unknown_additions, 0);
    // Foreign prompt/session recovery is separate from the blame-hunk lookup:
    // six AI sessions plus the two boundary-human lookups remain unchanged.
    assert_note_read_budget(&log, 8, 7);
}
