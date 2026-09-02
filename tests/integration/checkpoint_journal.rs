use crate::repos::test_repo::TestRepo;
use git_ai::error::GitAiError;
use git_ai::model::attribution::Attribution;
use git_ai::model::working_log::{Checkpoint, CheckpointKind, WorkingLogEntry};
use std::fs::{self, OpenOptions};
use std::io::Write;

fn checkpoint_file(repo: &TestRepo) -> std::path::PathBuf {
    repo.current_working_logs().dir.join("checkpoints.jsonl")
}

fn append_ai_checkpoint(repo: &TestRepo, content: &str) {
    fs::write(repo.path().join("sample.txt"), content).expect("write checkpoint fixture");
    repo.git_ai(&["checkpoint", "mock_ai", "sample.txt"])
        .expect("AI checkpoint should succeed");
}

fn attributed_checkpoint(repo: &TestRepo, author: &str) -> Checkpoint {
    let blob_sha = repo
        .current_working_logs()
        .persist_file_version(&format!("{author} blob\n"))
        .unwrap();
    Checkpoint::new(
        CheckpointKind::AiAgent,
        format!("{author}-diff"),
        author.to_string(),
        vec![WorkingLogEntry::new(
            "sample.txt".to_string(),
            blob_sha,
            vec![Attribution {
                start: 0,
                end: 1,
                author_id: author.to_string(),
                ts: 1,
            }],
            Vec::new(),
        )],
    )
}

#[test]
fn checkpoint_journal_appends_one_checksummed_record_without_rewriting_prefix() {
    let repo = TestRepo::new();
    append_ai_checkpoint(&repo, "first AI state\n");

    let path = checkpoint_file(&repo);
    let first_bytes = fs::read(&path).expect("read first checkpoint record");
    append_ai_checkpoint(&repo, "second AI state\n");
    let appended = fs::read(&path).expect("read appended checkpoint records");

    assert!(
        appended.starts_with(&first_bytes),
        "an ordinary checkpoint must retain the exact durable prefix"
    );
    let lines = appended
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty());
    assert_eq!(lines.count(), 2);

    let records = String::from_utf8(appended)
        .expect("checkpoint journal should be UTF-8")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    for record in records {
        assert_eq!(record["_git_ai_record_version"], 1);
        assert!(record["_git_ai_record_checksum"].as_str().is_some());
        assert!(record.get("_git_ai_blobs").is_none());
    }

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    let ai_checkpoints = checkpoints
        .iter()
        .filter(|checkpoint| checkpoint.kind == CheckpointKind::AiAgent)
        .collect::<Vec<_>>();
    assert_eq!(ai_checkpoints.len(), 2);
    assert!(ai_checkpoints[0].entries[0].attributions.is_empty());
    assert!(!ai_checkpoints[1].entries[0].attributions.is_empty());
    assert_eq!(
        repo.current_working_logs()
            .get_file_version(&ai_checkpoints[1].entries[0].blob_sha)
            .unwrap(),
        "second AI state\n"
    );
}

#[test]
fn checkpoint_journal_does_not_publish_an_index_record_when_blob_sync_fails() {
    let repo = TestRepo::new();
    let working_log = repo.current_working_logs();
    let mut checkpoints = Vec::new();
    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "durable"),
            0,
        )
        .unwrap();
    let prefix = fs::read(checkpoint_file(&repo)).unwrap();

    let checkpoint = attributed_checkpoint(&repo, "missing");
    let blob_sha = checkpoint.entries[0].blob_sha.clone();
    let blob_path = working_log.dir.join("blobs").join(&blob_sha);
    fs::remove_file(blob_path).unwrap();

    let error = working_log
        .append_checkpoint_record_with_compaction_interval_for_test(&mut checkpoints, checkpoint, 0)
        .expect_err("missing blob must fail before index publication");

    assert!(
        matches!(error, GitAiError::IoError(ref error) if error.kind() == std::io::ErrorKind::NotFound),
        "unexpected error: {error}"
    );
    assert_eq!(fs::read(checkpoint_file(&repo)).unwrap(), prefix);
}

#[test]
fn checkpoint_journal_discards_an_incomplete_final_record() {
    let mut repo = TestRepo::new_dedicated_daemon();
    append_ai_checkpoint(&repo, "durable AI state\n");

    let path = checkpoint_file(&repo);
    let durable_prefix = fs::read(&path).expect("read durable prefix");
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(br#"{"_git_ai_record_version":1,"kind":"AiAgent"#)
        .unwrap();

    repo.restart_dedicated_daemon_for_test();

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), durable_prefix);
}

#[test]
fn checkpoint_journal_survives_restart_and_accepts_the_next_checkpoint() {
    let mut repo = TestRepo::new_dedicated_daemon();
    append_ai_checkpoint(&repo, "first durable AI state\n");
    let first = repo.current_working_logs().read_all_checkpoints().unwrap();
    let first_blob = first[0].entries[0].blob_sha.clone();

    repo.restart_dedicated_daemon_for_test();
    append_ai_checkpoint(&repo, "second durable AI state\n");

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    assert_eq!(checkpoints.len(), 2);
    assert_eq!(
        repo.current_working_logs()
            .get_file_version(&first_blob)
            .unwrap(),
        "first durable AI state\n"
    );
}

#[test]
fn checkpoint_journal_discards_a_complete_versioned_record_without_newline() {
    let mut repo = TestRepo::new_dedicated_daemon();
    append_ai_checkpoint(&repo, "durable AI state\n");

    let path = checkpoint_file(&repo);
    let durable_prefix = fs::read(&path).expect("read durable prefix");
    let unterminated = durable_prefix
        .strip_suffix(b"\n")
        .expect("journal record should be newline terminated");
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(unterminated)
        .unwrap();

    repo.restart_dedicated_daemon_for_test();

    let checkpoints = repo.current_working_logs().read_all_checkpoints().unwrap();
    assert_eq!(checkpoints.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), durable_prefix);
}

#[test]
fn checkpoint_journal_keeps_a_complete_legacy_record_without_newline() {
    let repo = TestRepo::new();
    let working_log = repo.current_working_logs();
    let checkpoint = attributed_checkpoint(&repo, "legacy");
    fs::write(
        working_log.checkpoints_file(),
        serde_json::to_vec(&checkpoint).unwrap(),
    )
    .unwrap();

    let checkpoints = working_log.read_all_checkpoints().unwrap();

    assert_eq!(checkpoints.len(), 1);
    assert_eq!(checkpoints[0].author, "legacy");
}

#[test]
fn checkpoint_journal_terminates_a_complete_legacy_tail_before_append() {
    let repo = TestRepo::new();
    let working_log = repo.current_working_logs();
    let path = working_log.checkpoints_file();
    fs::write(
        &path,
        serde_json::to_vec(&attributed_checkpoint(&repo, "legacy")).unwrap(),
    )
    .unwrap();

    let mut checkpoints = working_log.read_all_checkpoints().unwrap();
    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "journal"),
            0,
        )
        .unwrap();

    assert_eq!(
        fs::read_to_string(&path).unwrap().lines().count(),
        2,
        "the first journal append must not concatenate onto a legacy tail"
    );
    assert_eq!(working_log.read_all_checkpoints().unwrap().len(), 2);
}

#[test]
fn checkpoint_journal_discards_an_unterminated_whitespace_tail_before_append() {
    let repo = TestRepo::new();
    let working_log = repo.current_working_logs();
    let mut checkpoints = Vec::new();
    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "first"),
            0,
        )
        .unwrap();
    let path = working_log.checkpoints_file();
    let durable_prefix = fs::read(&path).unwrap();
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(b"   ")
        .unwrap();

    let mut checkpoints = working_log.read_all_checkpoints().unwrap();
    assert_eq!(fs::read(&path).unwrap(), durable_prefix);
    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "second"),
            0,
        )
        .unwrap();

    assert_eq!(working_log.read_all_checkpoints().unwrap().len(), 2);
}

#[test]
fn checkpoint_journal_checksum_rejects_a_tampered_complete_record() {
    let repo = TestRepo::new();
    append_ai_checkpoint(&repo, "original AI state\n");

    let path = checkpoint_file(&repo);
    let original = fs::read_to_string(&path).unwrap();
    let tampered = original.replace("mock_ai", "tampered-agent");
    assert_ne!(tampered, original);
    fs::write(&path, tampered).unwrap();

    let error = repo
        .current_working_logs()
        .read_all_checkpoints()
        .expect_err("a complete record with a mismatched checksum must fail closed");
    assert!(
        error.to_string().contains("checksum"),
        "unexpected error: {error}"
    );
}

#[test]
fn checkpoint_journal_periodically_compacts_pruned_state() {
    let repo = TestRepo::new();
    let working_log = repo.current_working_logs();
    let path = working_log.checkpoints_file();
    let mut checkpoints = Vec::new();

    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "first"),
            2,
        )
        .unwrap();
    let first_record = fs::read(&path).unwrap();

    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "second"),
            2,
        )
        .unwrap();
    let compacted = fs::read(&path).unwrap();
    assert!(
        !compacted.starts_with(&first_record),
        "the compaction boundary must persist the pruned first checkpoint"
    );

    let compacted_checkpoints = working_log.read_all_checkpoints().unwrap();
    assert!(compacted_checkpoints[0].entries[0].attributions.is_empty());
    assert!(!compacted_checkpoints[1].entries[0].attributions.is_empty());

    working_log
        .append_checkpoint_record_with_compaction_interval_for_test(
            &mut checkpoints,
            attributed_checkpoint(&repo, "third"),
            2,
        )
        .unwrap();
    assert!(
        fs::read(&path).unwrap().starts_with(&compacted),
        "ordinary writes after compaction must return to one-record append"
    );
}
