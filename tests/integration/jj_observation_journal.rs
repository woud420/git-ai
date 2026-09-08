//! Synthetic raw bytes exercise capture integrity only; they are deliberately not
//! jj protobufs or content-addressed operation objects. The native reader must
//! establish those properties before any captured evidence can drive attribution.

use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use git_ai::model::jj_observation::{
    JJ_OBSERVATION_READER_PROFILE, JJ_OBSERVATION_SCHEMA_VERSION, JjObservationBatch,
    JjOperationEvidence, MAX_JJ_OBSERVATION_BATCH_BYTES, MAX_JJ_OBSERVATION_HEADS,
    MAX_JJ_OBSERVATION_OPERATION_BYTES, MAX_JJ_OBSERVATION_OPERATIONS,
};
use git_ai::model::repository::jj_observation_journal::{
    CaptureOutcome, JjObservationJournal, MAX_JJ_OBSERVATION_PENDING_LIMIT,
};
use git_ai::model::repository::sqlite::open_with_memory_limits;
use rusqlite::params;
use std::path::PathBuf;
use std::process::Command;

const CHILD_DB_ENV: &str = "GIT_AI_JJ_JOURNAL_CRASH_DB";
const CHILD_SOURCE_ENV: &str = "GIT_AI_JJ_JOURNAL_CRASH_SOURCE";

fn operation_id(number: u64) -> String {
    format!("{number:0128x}")
}

fn source_id(number: u64) -> String {
    format!("{number:064x}")
}

fn evidence(number: u64, parents: &[u64]) -> JjOperationEvidence {
    JjOperationEvidence {
        operation_id: operation_id(number),
        parent_ids: parents.iter().copied().map(operation_id).collect(),
        view_id: operation_id(number + 10_000),
        operation_bytes: format!("opaque operation {number}").into_bytes(),
        view_bytes: format!("opaque view {number}").into_bytes(),
    }
}

fn batch(
    source: &str,
    generation: u64,
    expected: &[u64],
    heads: &[u64],
    operations: Vec<JjOperationEvidence>,
) -> JjObservationBatch {
    JjObservationBatch {
        schema_version: JJ_OBSERVATION_SCHEMA_VERSION,
        reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
        source_id: source.to_owned(),
        expected_generation: generation,
        expected_observed_heads: expected.iter().copied().map(operation_id).collect(),
        captured_integrated_heads: heads.iter().copied().map(operation_id).collect(),
        operations,
    }
}

fn first_batch(source: &str) -> JjObservationBatch {
    batch(source, 0, &[], &[1], vec![evidence(1, &[0])])
}

struct Fixture {
    _repo: TestRepo,
    path: PathBuf,
    source: String,
}

impl Fixture {
    fn new() -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let path = repo.test_home_path().join("jj-observation-journal.sqlite");
        Self {
            _repo: repo,
            path,
            source: source_id(1),
        }
    }

    fn open(&self) -> JjObservationJournal {
        JjObservationJournal::open_at_path(&self.path).expect("open isolated journal")
    }

    fn capture_first(&self, journal: &mut JjObservationJournal) {
        assert_eq!(
            journal.capture(&first_batch(&self.source)).unwrap(),
            CaptureOutcome::Captured {
                inserted_operations: 1
            }
        );
    }

    fn assert_only_first(&self, journal: &JjObservationJournal) {
        let status = journal.status(&self.source).unwrap();
        assert_eq!(status.generation, 1);
        assert_eq!(status.observed_heads, vec![operation_id(1)]);
        assert!(status.applied_heads.is_empty());
        assert_eq!(status.pending_operations, 1);
        assert_eq!(
            journal.pending(&self.source, 10).unwrap(),
            vec![evidence(1, &[0])]
        );
    }
}

fn rejected<T, E: std::fmt::Display>(result: Result<T, E>, category: &str) {
    let error = match result {
        Ok(_) => panic!("expected {category} rejection"),
        Err(error) => error,
    };
    let diagnostic = error.to_string().to_ascii_lowercase();
    assert!(
        diagnostic.contains(category),
        "expected {category}: {diagnostic}"
    );
}

#[test]
fn jj_journal_capture_survives_reopen_without_marking_attribution_applied() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let fresh = journal.status(&fixture.source).unwrap();
    assert_eq!(fresh.generation, 0);
    assert!(fresh.observed_heads.is_empty());
    assert!(fresh.applied_heads.is_empty());
    assert_eq!(fresh.pending_operations, 0);
    fixture.capture_first(&mut journal);
    drop(journal);
    fixture.assert_only_first(&fixture.open());
}

#[test]
fn jj_journal_identical_capture_is_idempotent_before_and_after_reopen() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    assert_eq!(
        journal.capture(&first_batch(&fixture.source)).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    drop(journal);
    let mut reopened = fixture.open();
    assert_eq!(
        reopened.capture(&first_batch(&fixture.source)).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    fixture.assert_only_first(&reopened);
}

#[test]
fn jj_journal_historical_retry_cannot_rewind_a_later_observed_frontier() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let second = batch(&fixture.source, 1, &[1], &[2], vec![evidence(2, &[1])]);
    journal.capture(&second).unwrap();
    drop(journal);
    let mut reopened = fixture.open();
    assert_eq!(
        reopened.capture(&first_batch(&fixture.source)).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    let status = reopened.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 2);
    assert_eq!(status.observed_heads, vec![operation_id(2)]);
    assert!(status.applied_heads.is_empty());
    assert_eq!(status.pending_operations, 2);
}

#[test]
fn jj_journal_accepts_child_first_input_but_replays_parents_first() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let capture = batch(
        &fixture.source,
        0,
        &[],
        &[2],
        vec![evidence(2, &[1]), evidence(1, &[0])],
    );
    journal.capture(&capture).unwrap();
    assert_eq!(
        journal.pending(&fixture.source, 10).unwrap(),
        vec![evidence(1, &[0]), evidence(2, &[1])]
    );
}

#[test]
fn jj_journal_preserves_divergent_heads_until_an_explicit_join() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let diverged = batch(
        &fixture.source,
        1,
        &[1],
        &[3, 2],
        vec![evidence(3, &[1]), evidence(2, &[1])],
    );
    journal.capture(&diverged).unwrap();
    let mut heads = journal.status(&fixture.source).unwrap().observed_heads;
    heads.sort();
    assert_eq!(heads, vec![operation_id(2), operation_id(3)]);
    let joined = batch(
        &fixture.source,
        2,
        &[2, 3],
        &[4],
        vec![evidence(4, &[2, 3])],
    );
    journal.capture(&joined).unwrap();
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 3);
    assert_eq!(status.observed_heads, vec![operation_id(4)]);
    assert!(status.applied_heads.is_empty());
    assert_eq!(status.pending_operations, 4);
}

#[test]
fn jj_journal_missing_parent_does_not_advance_or_partially_append() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let incomplete = batch(
        &fixture.source,
        1,
        &[1],
        &[2, 3],
        vec![evidence(2, &[1]), evidence(3, &[999])],
    );
    rejected(journal.capture(&incomplete), "gap");
    fixture.assert_only_first(&journal);
    fixture.assert_only_first(&fixture.open());
}

#[test]
fn jj_journal_sql_write_failure_rolls_back_records_frontier_and_batch_receipt() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch(&format!(
        "CREATE TRIGGER reject_jj_operation BEFORE INSERT ON jj_operations
         WHEN NEW.operation_id = '{}' BEGIN
         SELECT RAISE(FAIL, 'injected journal write failure'); END;",
        operation_id(3)
    ))
    .unwrap();
    drop(conn);
    let capture = batch(
        &fixture.source,
        1,
        &[1],
        &[3],
        vec![evidence(2, &[1]), evidence(3, &[2])],
    );
    rejected(journal.capture(&capture), "injected journal write failure");
    fixture.assert_only_first(&journal);
    fixture.assert_only_first(&fixture.open());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute_batch("DROP TRIGGER reject_jj_operation")
        .unwrap();
    drop(conn);
    assert_eq!(
        journal.capture(&capture).unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 2
        }
    );
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        3
    );
}

#[test]
fn jj_journal_detached_extra_cannot_be_smuggled_into_a_captured_batch() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let detached = batch(
        &fixture.source,
        1,
        &[1],
        &[2],
        vec![evidence(2, &[1]), evidence(3, &[1])],
    );
    rejected(journal.capture(&detached), "reachable");
    fixture.assert_only_first(&journal);
}

#[test]
fn jj_journal_cycle_is_rejected_without_creating_source_progress() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let cyclic = batch(
        &fixture.source,
        0,
        &[],
        &[2],
        vec![evidence(2, &[3]), evidence(3, &[2])],
    );
    rejected(journal.capture(&cyclic), "cycle");
    let status = journal.status(&fixture.source).unwrap();
    assert!(status.observed_heads.is_empty());
    assert_eq!(status.pending_operations, 0);
}

#[test]
fn jj_journal_stale_frontier_is_rejected_without_losing_concurrent_progress() {
    let fixture = Fixture::new();
    let mut first_writer = fixture.open();
    let mut second_writer = fixture.open();
    fixture.capture_first(&mut first_writer);
    let stale = batch(&fixture.source, 1, &[], &[2], vec![evidence(2, &[0])]);
    rejected(second_writer.capture(&stale), "frontier");
    fixture.assert_only_first(&first_writer);
    fixture.assert_only_first(&second_writer);
}

#[test]
fn jj_journal_stale_generation_is_rejected_even_when_the_head_set_matches() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let mut stale = batch(&fixture.source, 0, &[1], &[2], vec![evidence(2, &[1])]);
    rejected(journal.capture(&stale), "generation");
    fixture.assert_only_first(&journal);
    stale.expected_generation = 1;
    journal.capture(&stale).unwrap();
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 2);
}

#[test]
fn jj_journal_late_branch_can_join_a_known_operation_behind_the_current_frontier() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let earlier = batch(
        &fixture.source,
        0,
        &[],
        &[2],
        vec![evidence(1, &[0]), evidence(2, &[1])],
    );
    journal.capture(&earlier).unwrap();
    let late_branch = batch(&fixture.source, 1, &[2], &[2, 3], vec![evidence(3, &[1])]);
    assert_eq!(
        journal.capture(&late_branch).unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 1
        }
    );
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(
        status.observed_heads,
        vec![operation_id(2), operation_id(3)]
    );
    assert_eq!(status.pending_operations, 3);
    assert!(status.applied_heads.is_empty());
}

#[test]
fn jj_journal_redundant_head_changes_need_no_new_operation_records() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let earlier = batch(
        &fixture.source,
        0,
        &[],
        &[2],
        vec![evidence(1, &[0]), evidence(2, &[1])],
    );
    journal.capture(&earlier).unwrap();
    let redundant = batch(&fixture.source, 1, &[2], &[1, 2], vec![]);
    journal.capture(&redundant).unwrap();
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 2);
    assert_eq!(
        status.observed_heads,
        vec![operation_id(1), operation_id(2)]
    );
    assert_eq!(status.pending_operations, 2);
    let cleaned_up = batch(&fixture.source, 2, &[1, 2], &[2], vec![]);
    journal.capture(&cleaned_up).unwrap();
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 3);
    assert_eq!(status.observed_heads, vec![operation_id(2)]);
    assert_eq!(status.pending_operations, 2);
    assert!(status.applied_heads.is_empty());
    assert_eq!(
        journal.capture(&redundant).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    assert_eq!(
        journal.status(&fixture.source).unwrap().observed_heads,
        vec![operation_id(2)]
    );
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 3);

    let recurring_redundant = batch(&fixture.source, 3, &[2], &[1, 2], vec![]);
    assert_eq!(
        journal.capture(&recurring_redundant).unwrap(),
        CaptureOutcome::Captured {
            inserted_operations: 0
        }
    );
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 4);
    assert_eq!(
        status.observed_heads,
        vec![operation_id(1), operation_id(2)]
    );
    assert_eq!(
        journal.capture(&cleaned_up).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 4);
    let recurring_cleanup = batch(&fixture.source, 4, &[1, 2], &[2], vec![]);
    journal.capture(&recurring_cleanup).unwrap();
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 5);
    assert_eq!(status.observed_heads, vec![operation_id(2)]);
    assert_eq!(status.pending_operations, 2);
    assert!(status.applied_heads.is_empty());
}

#[test]
fn jj_journal_empty_batch_cannot_advance_to_an_unknown_head() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let missing_head = batch(&fixture.source, 1, &[1], &[999], vec![]);
    rejected(journal.capture(&missing_head), "gap");
    fixture.assert_only_first(&journal);
}

#[test]
fn jj_journal_same_operation_id_with_changed_evidence_rolls_back_whole_batch() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    for field in ["operation", "view", "parents", "view_id"] {
        let mut changed = evidence(1, &[0]);
        match field {
            "operation" => changed.operation_bytes.push(b'!'),
            "view" => changed.view_bytes.push(b'!'),
            "parents" => changed.parent_ids = vec![operation_id(999)],
            "view_id" => changed.view_id = operation_id(999),
            _ => unreachable!(),
        }
        let collision = batch(
            &fixture.source,
            1,
            &[1],
            &[2],
            vec![evidence(2, &[1]), changed],
        );
        rejected(journal.capture(&collision), "collision");
        fixture.assert_only_first(&journal);
    }
    fixture.assert_only_first(&fixture.open());
}

#[test]
fn jj_journal_rejects_duplicate_heads_operations_and_parents() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut duplicate_heads = first_batch(&fixture.source);
    duplicate_heads
        .captured_integrated_heads
        .push(operation_id(1));
    rejected(journal.capture(&duplicate_heads), "duplicate");
    let mut duplicate_operations = first_batch(&fixture.source);
    duplicate_operations.operations.push(evidence(1, &[0]));
    rejected(journal.capture(&duplicate_operations), "duplicate");
    let duplicate_parents = batch(&fixture.source, 0, &[], &[1], vec![evidence(1, &[0, 0])]);
    rejected(journal.capture(&duplicate_parents), "duplicate");
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        0
    );
}

#[test]
fn jj_journal_root_sentinel_is_a_boundary_not_a_storable_operation() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let root = batch(&fixture.source, 0, &[], &[0], vec![evidence(0, &[])]);
    rejected(journal.capture(&root), "root");
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        0
    );
}

#[test]
fn jj_journal_unknown_wire_version_and_malformed_identities_are_rejected() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut future = first_batch(&fixture.source);
    future.schema_version += 1;
    rejected(journal.capture(&future), "version");
    let mut unsupported = first_batch(&fixture.source);
    unsupported.reader_profile = "unknown-profile".to_string();
    rejected(journal.capture(&unsupported), "profile");
    for source in ["", "../store", "AA", &"A".repeat(64), &"a".repeat(65)] {
        rejected(journal.capture(&first_batch(source)), "source");
    }
    for invalid in [
        "",
        "abcd",
        "../operation",
        &"A".repeat(128),
        &"a".repeat(129),
    ] {
        let mut capture = first_batch(&fixture.source);
        capture.operations[0].operation_id = invalid.to_owned();
        rejected(journal.capture(&capture), "identity");
    }
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        0
    );
}

#[test]
fn jj_journal_operation_count_and_head_count_are_bounded() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let count = MAX_JJ_OBSERVATION_OPERATIONS + 1;
    let operations = (1..=count as u64)
        .map(|id| evidence(id, &[id - 1]))
        .collect();
    let too_many = batch(&fixture.source, 0, &[], &[count as u64], operations);
    rejected(journal.capture(&too_many), "limit");
    let mut heads = first_batch(&fixture.source);
    heads.captured_integrated_heads = (1..=MAX_JJ_OBSERVATION_HEADS as u64 + 1)
        .map(operation_id)
        .collect();
    rejected(journal.capture(&heads), "limit");
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        0
    );
}

#[test]
fn jj_journal_raw_operation_evidence_has_a_size_limit() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let mut capture = first_batch(&fixture.source);
    capture.operations[0].operation_bytes = vec![b'x'; MAX_JJ_OBSERVATION_OPERATION_BYTES + 1];
    rejected(journal.capture(&capture), "limit");
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        0
    );
}

#[test]
fn jj_journal_aggregate_encoded_payload_is_bounded() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let piece_size = MAX_JJ_OBSERVATION_OPERATION_BYTES / 2;
    let count = MAX_JJ_OBSERVATION_BATCH_BYTES / piece_size + 1;
    assert!(count <= MAX_JJ_OBSERVATION_OPERATIONS);
    let operations = (1..=count as u64)
        .map(|id| {
            let mut operation = evidence(id, &[id - 1]);
            operation.operation_bytes = vec![b'x'; piece_size];
            operation
        })
        .collect();
    let capture = batch(&fixture.source, 0, &[], &[count as u64], operations);
    rejected(journal.capture(&capture), "limit");
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        0
    );
}

#[test]
fn jj_journal_pending_reads_obey_the_requested_page_limit() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let capture = batch(
        &fixture.source,
        0,
        &[],
        &[3],
        vec![evidence(3, &[2]), evidence(1, &[0]), evidence(2, &[1])],
    );
    journal.capture(&capture).unwrap();
    assert_eq!(
        journal.pending(&fixture.source, 1).unwrap(),
        vec![evidence(1, &[0])]
    );
    assert_eq!(journal.pending(&fixture.source, 2).unwrap().len(), 2);
    rejected(journal.pending(&fixture.source, 0), "limit");
    rejected(
        journal.pending(&fixture.source, MAX_JJ_OBSERVATION_PENDING_LIMIT + 1),
        "limit",
    );
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        3
    );
}

#[test]
fn jj_journal_distinct_sources_cannot_share_progress_or_collision_state() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let other = source_id(2);
    let mut other_capture = first_batch(&other);
    other_capture.operations[0].operation_bytes = b"different store evidence".to_vec();
    journal.capture(&other_capture).unwrap();
    fixture.assert_only_first(&journal);
    assert_eq!(
        journal.pending(&other, 10).unwrap(),
        other_capture.operations
    );
    assert!(journal.status(&other).unwrap().applied_heads.is_empty());
}

#[test]
fn jj_journal_corrupted_payload_fails_checksum_before_decode() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    drop(journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    assert_eq!(
        conn.execute(
            "UPDATE jj_operations SET payload = ?1 WHERE source_id = ?2 AND operation_id = ?3",
            params![
                b"not a decodable envelope".as_slice(),
                fixture.source,
                operation_id(1)
            ],
        )
        .unwrap(),
        1
    );
    drop(conn);
    let mut reopened = fixture.open();
    rejected(reopened.pending(&fixture.source, 10), "checksum");
    rejected(reopened.capture(&first_batch(&fixture.source)), "checksum");
    assert!(
        reopened
            .status(&fixture.source)
            .unwrap()
            .applied_heads
            .is_empty()
    );
}

#[test]
fn jj_journal_forward_and_malformed_schema_metadata_fail_closed() {
    for version in ["999", "garbled", "01"] {
        let fixture = Fixture::new();
        drop(fixture.open());
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute(
            "UPDATE schema_metadata SET value = ?1 WHERE key = 'version'",
            [version],
        )
        .unwrap();
        drop(conn);
        rejected(JjObservationJournal::open_at_path(&fixture.path), "schema");
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let persisted: String = conn
            .query_row(
                "SELECT value FROM schema_metadata WHERE key = 'version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            persisted, version,
            "opening must not repair unknown metadata"
        );
    }
}

#[test]
fn jj_journal_missing_version_on_existing_store_is_not_treated_as_fresh() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    drop(journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute("DELETE FROM schema_metadata WHERE key = 'version'", [])
        .unwrap();
    drop(conn);
    rejected(JjObservationJournal::open_at_path(&fixture.path), "schema");
}

#[test]
fn jj_journal_successful_capture_survives_process_exit_without_cleanup() {
    let fixture = Fixture::new();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "jj_observation_journal::capture_then_exit_without_closing_journal",
        ])
        .env(CHILD_DB_ENV, &fixture.path)
        .env(CHILD_SOURCE_ENV, &fixture.source)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(73),
        "child did not reach the post-commit exit: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.assert_only_first(&fixture.open());
}

#[test]
#[ignore = "subprocess crash helper; invoked by the process-exit durability test"]
fn capture_then_exit_without_closing_journal() {
    let Some(path) = std::env::var_os(CHILD_DB_ENV).map(PathBuf::from) else {
        return;
    };
    let source = std::env::var(CHILD_SOURCE_ENV).expect("child source identity");
    let mut journal = JjObservationJournal::open_at_path(&path).unwrap();
    journal.capture(&first_batch(&source)).unwrap();
    // Skipping Connection::drop preserves any committed WAL as it was when the
    // capture returned. This exercises process recovery, not power-loss safety.
    std::process::exit(73);
}

#[test]
fn jj_journal_view_identity_collision_is_independent_of_operation_identity() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let original = evidence(1, &[0]);
    let mut shared = evidence(2, &[1]);
    shared.view_id = original.view_id;
    shared.view_bytes = original.view_bytes;
    journal
        .capture(&batch(&fixture.source, 1, &[1], &[2], vec![shared.clone()]))
        .unwrap();
    let mut conflicting = evidence(3, &[2]);
    conflicting.view_id = shared.view_id;
    conflicting.view_bytes = b"different bytes for the same immutable view".to_vec();
    rejected(
        journal.capture(&batch(&fixture.source, 2, &[2], &[3], vec![conflicting])),
        "collision",
    );
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 2);
    assert_eq!(status.observed_heads, vec![operation_id(2)]);
    assert_eq!(status.pending_operations, 2);
}

#[test]
fn jj_journal_independent_writers_cannot_acknowledge_the_same_generation() {
    let fixture = Fixture::new();
    let mut winner = fixture.open();
    fixture.capture_first(&mut winner);
    let mut loser = fixture.open();
    winner
        .capture(&batch(
            &fixture.source,
            1,
            &[1],
            &[2],
            vec![evidence(2, &[1])],
        ))
        .unwrap();
    rejected(
        loser.capture(&batch(
            &fixture.source,
            1,
            &[1],
            &[3],
            vec![evidence(3, &[1])],
        )),
        "generation",
    );
    let reopened = fixture.open();
    let status = reopened.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 2);
    assert_eq!(status.observed_heads, vec![operation_id(2)]);
    assert_eq!(
        reopened.pending(&fixture.source, 10).unwrap(),
        vec![evidence(1, &[0]), evidence(2, &[1])]
    );
}

#[test]
fn jj_journal_oversized_stored_payload_is_rejected_before_decoding() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET payload = zeroblob(?1) WHERE source_id = ?2",
        params![MAX_JJ_OBSERVATION_BATCH_BYTES + 1, fixture.source],
    )
    .unwrap();
    rejected(journal.pending(&fixture.source, 1), "limit");
}

#[test]
fn jj_journal_pending_page_is_bounded_across_separate_captures() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let piece_size = MAX_JJ_OBSERVATION_OPERATION_BYTES / 2;
    let count = MAX_JJ_OBSERVATION_BATCH_BYTES / piece_size + 1;
    assert!(count <= MAX_JJ_OBSERVATION_PENDING_LIMIT);
    for number in 1..=count as u64 {
        let mut record = evidence(number, &[number - 1]);
        record.operation_bytes = vec![b'x'; piece_size];
        let expected = if number == 1 {
            vec![]
        } else {
            vec![number - 1]
        };
        journal
            .capture(&batch(
                &fixture.source,
                number - 1,
                &expected,
                &[number],
                vec![record],
            ))
            .unwrap();
    }
    assert_eq!(journal.pending(&fixture.source, 1).unwrap().len(), 1);
    rejected(journal.pending(&fixture.source, count), "limit");
}

#[test]
fn jj_journal_corrupted_boundary_cannot_authorize_new_children_or_heads() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET payload = ?1 WHERE source_id = ?2",
        params![b"broken evidence".as_slice(), fixture.source],
    )
    .unwrap();
    rejected(
        journal.capture(&batch(
            &fixture.source,
            1,
            &[1],
            &[2],
            vec![evidence(2, &[1])],
        )),
        "checksum",
    );
    rejected(
        journal.capture(&batch(&fixture.source, 1, &[1], &[1], vec![])),
        "checksum",
    );
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 1);
    assert_eq!(status.observed_heads, vec![operation_id(1)]);
    assert_eq!(status.pending_operations, 1);
}

#[test]
fn jj_journal_known_parent_membership_is_scoped_to_its_source() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let other = source_id(2);
    rejected(
        journal.capture(&batch(&other, 0, &[], &[2], vec![evidence(2, &[1])])),
        "gap",
    );
    let status = journal.status(&other).unwrap();
    assert_eq!(status.generation, 0);
    assert!(status.observed_heads.is_empty());
    fixture.assert_only_first(&journal);
}

#[test]
fn jj_journal_missing_head_cannot_be_hidden_by_an_empty_historical_receipt() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let observation = batch(&fixture.source, 1, &[1], &[1], vec![]);
    journal.capture(&observation).unwrap();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "DELETE FROM jj_views WHERE source_id = ?1",
        [&fixture.source],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM jj_operations WHERE source_id = ?1",
        [&fixture.source],
    )
    .unwrap();
    rejected(journal.capture(&observation), "gap");
    let status = journal.status(&fixture.source).unwrap();
    assert_eq!(status.generation, 2);
    assert_eq!(status.observed_heads, vec![operation_id(1)]);
    assert!(status.applied_heads.is_empty());
}

#[test]
fn jj_journal_missing_parent_behind_the_frontier_invalidates_historical_receipt() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    journal
        .capture(&batch(
            &fixture.source,
            0,
            &[],
            &[2],
            vec![evidence(1, &[0]), evidence(2, &[1])],
        ))
        .unwrap();
    let late = batch(&fixture.source, 1, &[2], &[2, 3], vec![evidence(3, &[1])]);
    journal.capture(&late).unwrap();
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "DELETE FROM jj_views WHERE source_id = ?1 AND operation_id = ?2",
        params![fixture.source, operation_id(1)],
    )
    .unwrap();
    conn.execute(
        "DELETE FROM jj_operations WHERE source_id = ?1 AND operation_id = ?2",
        params![fixture.source, operation_id(1)],
    )
    .unwrap();
    rejected(journal.capture(&late), "gap");
    assert_eq!(journal.status(&fixture.source).unwrap().generation, 2);
}

#[test]
fn jj_journal_missing_pending_records_are_not_silently_skipped() {
    for missing in [1, 2, 3] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        journal
            .capture(&batch(
                &fixture.source,
                0,
                &[],
                &[3],
                vec![evidence(1, &[0]), evidence(2, &[1]), evidence(3, &[2])],
            ))
            .unwrap();
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.execute(
            "DELETE FROM jj_views WHERE source_id = ?1 AND operation_id = ?2",
            params![fixture.source, operation_id(missing)],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM jj_operations WHERE source_id = ?1 AND operation_id = ?2",
            params![fixture.source, operation_id(missing)],
        )
        .unwrap();
        rejected(journal.pending(&fixture.source, 3), "gap");
    }
}

#[test]
fn jj_journal_large_accepted_batch_can_always_be_retried_with_its_existing_boundary() {
    fn large(number: u64, parent: u64) -> JjOperationEvidence {
        let mut record = evidence(number, &[parent]);
        record.operation_bytes =
            vec![255; MAX_JJ_OBSERVATION_OPERATION_BYTES - record.view_bytes.len()];
        record
    }
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    journal
        .capture(&batch(&fixture.source, 0, &[], &[1], vec![large(1, 0)]))
        .unwrap();
    let next = batch(
        &fixture.source,
        1,
        &[1],
        &[4],
        vec![large(2, 1), large(3, 2), large(4, 3)],
    );
    journal.capture(&next).unwrap();
    assert_eq!(
        journal.capture(&next).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    drop(journal);
    let mut reopened = fixture.open();
    assert_eq!(
        reopened.capture(&next).unwrap(),
        CaptureOutcome::AlreadyCaptured
    );
    assert_eq!(reopened.status(&fixture.source).unwrap().generation, 2);
}

#[test]
fn jj_journal_lookup_empty_source_returns_no_observed_evidence_or_progress() {
    let fixture = Fixture::new();
    let journal = fixture.open();
    for ids in [vec![], vec![operation_id(1), operation_id(2)]] {
        let observed = journal.lookup_observed(&fixture.source, &ids).unwrap();
        assert!(observed.operations.is_empty());
        assert_eq!(observed.status, journal.status(&fixture.source).unwrap());
        assert_eq!(observed.status.generation, 0);
        assert_eq!(observed.status.pending_operations, 0);
        assert!(observed.status.observed_heads.is_empty());
        assert!(observed.status.applied_heads.is_empty());
    }
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let count: u64 = conn
        .query_row("SELECT count(*) FROM jj_sources", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn jj_journal_lookup_returns_only_requested_records_and_preserves_unknown_ids() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let expected_status = journal.status(&fixture.source).unwrap();
    let observed = journal
        .lookup_observed(&fixture.source, &[operation_id(999), operation_id(1)])
        .unwrap();
    assert_eq!(observed.status, expected_status);
    assert_eq!(observed.operations.len(), 1);
    assert_eq!(
        observed.operations.get(&operation_id(1)),
        Some(&evidence(1, &[0]))
    );
    assert!(!observed.operations.contains_key(&operation_id(999)));
    let absent = journal
        .lookup_observed(&fixture.source, &[operation_id(998)])
        .unwrap();
    assert_eq!(absent.status, expected_status);
    assert!(absent.operations.is_empty());
    fixture.assert_only_first(&journal);
}

#[test]
fn jj_journal_lookup_recovers_an_old_ancestor_behind_divergent_heads_after_reopen() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    journal
        .capture(&batch(
            &fixture.source,
            0,
            &[],
            &[2],
            vec![evidence(1, &[0]), evidence(2, &[1])],
        ))
        .unwrap();
    journal
        .capture(&batch(
            &fixture.source,
            1,
            &[2],
            &[2, 3],
            vec![evidence(3, &[1])],
        ))
        .unwrap();
    drop(journal);
    let journal = fixture.open();
    let observed = journal
        .lookup_observed(&fixture.source, &[operation_id(1)])
        .unwrap();
    assert_eq!(
        observed.operations.get(&operation_id(1)),
        Some(&evidence(1, &[0]))
    );
    assert_eq!(
        observed.status.observed_heads,
        vec![operation_id(2), operation_id(3)]
    );
    assert_eq!(observed.status.generation, 2);
    assert_eq!(observed.status.pending_operations, 3);
    assert!(observed.status.applied_heads.is_empty());
}

#[test]
fn jj_journal_lookup_can_address_records_beyond_the_pending_prefix() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    let count = MAX_JJ_OBSERVATION_PENDING_LIMIT as u64 + 1;
    journal
        .capture(&batch(
            &fixture.source,
            0,
            &[],
            &[count],
            (1..=count)
                .map(|number| evidence(number, &[number - 1]))
                .collect(),
        ))
        .unwrap();
    let observed = journal
        .lookup_observed(&fixture.source, &[operation_id(count), operation_id(1)])
        .unwrap();
    assert_eq!(observed.operations.len(), 2);
    assert_eq!(
        observed.operations.get(&operation_id(count)),
        Some(&evidence(count, &[count - 1]))
    );
    assert_eq!(
        observed.operations.get(&operation_id(1)),
        Some(&evidence(1, &[0]))
    );
    assert_eq!(observed.status.pending_operations, count);
}

#[test]
fn jj_journal_lookup_membership_and_payloads_are_scoped_to_the_source() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let other_source = source_id(2);
    let mut other_evidence = evidence(1, &[0]);
    other_evidence.operation_bytes = b"different source's opaque operation".to_vec();
    other_evidence.view_bytes = b"different source's opaque view".to_vec();
    journal
        .capture(&batch(
            &other_source,
            0,
            &[],
            &[1],
            vec![other_evidence.clone()],
        ))
        .unwrap();
    for (source, expected) in [
        (fixture.source.as_str(), evidence(1, &[0])),
        (other_source.as_str(), other_evidence),
    ] {
        let observed = journal.lookup_observed(source, &[operation_id(1)]).unwrap();
        assert_eq!(observed.operations.get(&operation_id(1)), Some(&expected));
        assert_eq!(observed.status.generation, 1);
    }
    let absent = journal
        .lookup_observed(&source_id(3), &[operation_id(1)])
        .unwrap();
    assert!(absent.operations.is_empty());
    assert_eq!(absent.status.generation, 0);
}

#[test]
fn jj_journal_lookup_bounds_requested_id_count_before_database_reads() {
    use git_ai::model::repository::jj_observation_journal::MAX_JJ_OBSERVATION_LOOKUP_LIMIT;

    let fixture = Fixture::new();
    let journal = fixture.open();
    let maximum: Vec<_> = (1..=MAX_JJ_OBSERVATION_LOOKUP_LIMIT as u64)
        .map(operation_id)
        .collect();
    assert!(
        journal
            .lookup_observed(&fixture.source, &maximum)
            .unwrap()
            .operations
            .is_empty()
    );
    let mut too_many = maximum;
    too_many.push(operation_id(MAX_JJ_OBSERVATION_LOOKUP_LIMIT as u64 + 1));
    rejected(journal.lookup_observed(&fixture.source, &too_many), "limit");
}

#[test]
fn jj_journal_lookup_rejects_duplicate_malformed_and_synthetic_root_requests() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(1), operation_id(1)]),
        "duplicate",
    );
    for invalid in [
        "".to_owned(),
        "abcd".to_owned(),
        "../operation".to_owned(),
        "A".repeat(128),
        "a".repeat(129),
    ] {
        rejected(
            journal.lookup_observed(&fixture.source, &[invalid]),
            "identity",
        );
    }
    for invalid in [
        "".to_owned(),
        "../store".to_owned(),
        "A".repeat(64),
        "a".repeat(65),
    ] {
        rejected(
            journal.lookup_observed(&invalid, &[operation_id(1)]),
            "source",
        );
    }
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(0)]),
        "root",
    );
    fixture.assert_only_first(&journal);
}

#[test]
fn jj_journal_lookup_verifies_requested_record_checksums_before_reporting_membership() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET payload = ?1 WHERE source_id = ?2",
        params![b"broken evidence".as_slice(), fixture.source],
    )
    .unwrap();
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(1)]),
        "checksum",
    );
}

#[test]
fn jj_journal_lookup_rejects_oversized_records_before_decoding() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET payload = zeroblob(?1) WHERE source_id = ?2",
        params![MAX_JJ_OBSERVATION_BATCH_BYTES + 1, fixture.source],
    )
    .unwrap();
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(1)]),
        "limit",
    );
}

#[test]
fn jj_journal_lookup_checks_source_state_even_when_requested_ids_are_absent() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_sources SET state = ?1 WHERE source_id = ?2",
        params![b"broken progress".as_slice(), fixture.source],
    )
    .unwrap();
    for ids in [vec![], vec![operation_id(999)]] {
        rejected(journal.lookup_observed(&fixture.source, &ids), "checksum");
    }
}

#[test]
fn jj_journal_lookup_cannot_trust_an_operation_without_its_source_state() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute(
        "DELETE FROM jj_sources WHERE source_id = ?1",
        params![fixture.source],
    )
    .unwrap();
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(1)]),
        "gap",
    );
    let count: u64 = conn
        .query_row(
            "SELECT count(*) FROM jj_sources WHERE source_id = ?1",
            params![fixture.source],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn jj_journal_lookup_distinguishes_fresh_sources_from_orphaned_unrequested_records() {
    for orphan in ["operation", "receipt", "view"] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        fixture.capture_first(&mut journal);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        conn.pragma_update(None, "foreign_keys", false).unwrap();
        conn.execute(
            "DELETE FROM jj_sources WHERE source_id = ?1",
            params![fixture.source],
        )
        .unwrap();
        if orphan != "operation" {
            conn.execute(
                "DELETE FROM jj_operations WHERE source_id = ?1",
                params![fixture.source],
            )
            .unwrap();
        }
        if orphan != "receipt" {
            conn.execute(
                "DELETE FROM jj_batches WHERE source_id = ?1",
                params![fixture.source],
            )
            .unwrap();
        }
        if orphan != "view" {
            conn.execute(
                "DELETE FROM jj_views WHERE source_id = ?1",
                params![fixture.source],
            )
            .unwrap();
        }
        for ids in [vec![], vec![operation_id(999)]] {
            rejected(journal.lookup_observed(&fixture.source, &ids), "gap");
        }
    }
}

#[test]
fn jj_journal_lookup_rejects_records_beyond_the_stored_progress_count() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET sequence = 2 WHERE source_id = ?1",
        params![fixture.source],
    )
    .unwrap();
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(1)]),
        "gap",
    );
}

#[test]
fn jj_journal_lookup_aggregate_bytes_are_bounded_across_separate_captures() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    for number in 1..=4 {
        let mut record = evidence(number, &[number - 1]);
        record.operation_bytes = vec![255; MAX_JJ_OBSERVATION_OPERATION_BYTES];
        record.view_bytes.clear();
        let expected = if number == 1 {
            vec![]
        } else {
            vec![number - 1]
        };
        journal
            .capture(&batch(
                &fixture.source,
                number - 1,
                &expected,
                &[number],
                vec![record],
            ))
            .unwrap();
    }
    let before = journal.status(&fixture.source).unwrap();
    assert_eq!(
        journal
            .lookup_observed(
                &fixture.source,
                &[operation_id(1), operation_id(2), operation_id(3)]
            )
            .unwrap()
            .operations
            .len(),
        3,
    );
    rejected(
        journal.lookup_observed(
            &fixture.source,
            &[
                operation_id(1),
                operation_id(2),
                operation_id(3),
                operation_id(4),
            ],
        ),
        "limit",
    );
    assert_eq!(journal.status(&fixture.source).unwrap(), before);
}

#[test]
fn jj_journal_lookup_status_and_evidence_share_one_source_snapshot_during_capture() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};

    const COUNT: u64 = 64;
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let path = fixture.path.clone();
    let source = fixture.source.clone();
    let barrier = Arc::new(Barrier::new(2));
    let writer_barrier = Arc::clone(&barrier);
    let finished = Arc::new(AtomicBool::new(false));
    let writer_finished = Arc::clone(&finished);
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || {
            writer_barrier.wait();
            let result = (|| -> Result<(), String> {
                let mut journal =
                    JjObservationJournal::open_at_path(&path).map_err(|error| error.to_string())?;
                for number in 2..=COUNT {
                    journal
                        .capture(&batch(
                            &source,
                            number - 1,
                            &[number - 1],
                            &[number],
                            vec![evidence(number, &[number - 1])],
                        ))
                        .map_err(|error| error.to_string())?;
                    std::thread::yield_now();
                }
                Ok(())
            })();
            writer_finished.store(true, Ordering::Release);
            result
        });
        let ids: Vec<_> = (1..=COUNT).map(operation_id).collect();
        barrier.wait();
        let started = std::time::Instant::now();
        loop {
            let observed = journal.lookup_observed(&fixture.source, &ids).unwrap();
            let count = observed.status.pending_operations;
            assert!((1..=COUNT).contains(&count));
            assert_eq!(observed.status.generation, count);
            assert_eq!(observed.status.observed_heads, vec![operation_id(count)]);
            assert_eq!(observed.operations.len() as u64, count);
            assert!(observed.status.applied_heads.is_empty());
            for number in 1..=count {
                assert_eq!(
                    observed.operations.get(&operation_id(number)),
                    Some(&evidence(number, &[number - 1]))
                );
            }
            if finished.load(Ordering::Acquire) {
                break;
            }
            assert!(
                started.elapsed() < std::time::Duration::from_secs(10),
                "capture writer did not finish"
            );
            std::thread::yield_now();
        }
        writer.join().unwrap().unwrap();
        let observed = journal.lookup_observed(&fixture.source, &ids).unwrap();
        assert_eq!(observed.status.pending_operations, COUNT);
        assert_eq!(observed.operations.len() as u64, COUNT);
    });
}

#[test]
fn jj_journal_lookup_rejects_a_missing_requested_observed_head() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute(
        "DELETE FROM jj_operations WHERE source_id = ?1 AND operation_id = ?2",
        params![fixture.source, operation_id(1)],
    )
    .unwrap();
    let unknown = journal
        .lookup_observed(&fixture.source, &[operation_id(999)])
        .unwrap();
    assert!(unknown.operations.is_empty());
    assert_eq!(unknown.status.observed_heads, vec![operation_id(1)]);
    rejected(
        journal.lookup_observed(&fixture.source, &[operation_id(1)]),
        "gap",
    );
}
