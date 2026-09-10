use super::*;
use git_ai::model::repository::jj_observation_journal::{
    MAX_JJ_OBSERVATION_LOOKUP_LIMIT, ReadBudget,
};
use sha2::{Digest, Sha256};

fn stored_lengths(fixture: &Fixture, ids: &[u64]) -> (usize, Vec<usize>) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let state = conn
        .query_row(
            "SELECT length(state) FROM jj_sources WHERE source_id = ?1",
            [&fixture.source],
            |row| row.get::<_, usize>(0),
        )
        .unwrap();
    let mut statement = conn
        .prepare(
            "SELECT length(payload) FROM jj_operations
             WHERE source_id = ?1 AND operation_id = ?2",
        )
        .unwrap();
    let operations = ids
        .iter()
        .map(|id| {
            statement
                .query_row(params![fixture.source, operation_id(*id)], |row| {
                    row.get::<_, usize>(0)
                })
                .unwrap()
        })
        .collect();
    (state, operations)
}

fn capture_two(fixture: &Fixture, journal: &mut JjObservationJournal) {
    journal
        .capture(&batch(
            &fixture.source,
            0,
            &[],
            &[2],
            vec![evidence(1, &[0]), evidence(2, &[1])],
        ))
        .unwrap();
}

fn replace_payload(fixture: &Fixture, id: u64, payload: &[u8], checksum: &str) {
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET payload = ?1, checksum = ?2 WHERE source_id = ?3 AND operation_id = ?4",
        params![payload, checksum, fixture.source, operation_id(id)],
    ).unwrap();
}

#[test]
fn jj_journal_budget_fresh_source_can_answer_absence_without_blob_bytes() {
    let fixture = Fixture::new();
    let journal = fixture.open();
    let mut budget = ReadBudget::new(0);
    for ids in [vec![], vec![operation_id(1), operation_id(99)]] {
        let result = journal
            .lookup_observed_with_budget(&fixture.source, &ids, &mut budget)
            .unwrap();
        assert_eq!(result.status.generation, 0);
        assert!(result.operations.is_empty());
        assert_eq!(budget.consumed(), 0);
        assert_eq!(budget.remaining(), 0);
    }
}

#[test]
fn jj_journal_budget_charges_state_once_even_for_empty_or_unknown_lookup() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let (state, _) = stored_lengths(&fixture, &[]);
    for ids in [vec![], vec![operation_id(99)]] {
        let mut exact = ReadBudget::new(state);
        let result = journal
            .lookup_observed_with_budget(&fixture.source, &ids, &mut exact)
            .unwrap();
        assert_eq!(result.status, journal.status(&fixture.source).unwrap());
        assert!(result.operations.is_empty());
        assert_eq!(exact.consumed(), state);
        assert_eq!(exact.remaining(), 0);
        let mut short = ReadBudget::new(state - 1);
        rejected(
            journal.lookup_observed_with_budget(&fixture.source, &ids, &mut short),
            "byte limit",
        );
        assert_eq!(short.consumed(), 0);
        assert_eq!(short.remaining(), state - 1);
    }
}

#[test]
fn jj_journal_budget_is_inclusive_for_encoded_state_and_operation_blob() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let (state, payloads) = stored_lengths(&fixture, &[1]);
    let total = state + payloads[0];
    let raw = evidence(1, &[0]);
    assert!(payloads[0] > raw.operation_bytes.len() + raw.view_bytes.len());
    let mut budget = ReadBudget::new(total);
    let result = journal
        .lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget)
        .unwrap();
    assert_eq!(
        result,
        journal
            .lookup_observed(&fixture.source, &[operation_id(1)])
            .unwrap()
    );
    assert_eq!(result.operations[&operation_id(1)], raw);
    assert_eq!(budget.consumed(), total);
    assert_eq!(budget.remaining(), 0);
    let mut short = ReadBudget::new(total - 1);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut short),
        "byte limit",
    );
    assert_eq!(short.consumed(), state);
    assert_eq!(short.remaining(), payloads[0] - 1);
}

#[test]
fn jj_journal_budget_can_be_reused_across_independent_lookup_snapshots() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    capture_two(&fixture, &mut journal);
    let (state, payloads) = stored_lengths(&fixture, &[1, 2]);
    let total = 2 * state + payloads.iter().sum::<usize>();
    let mut budget = ReadBudget::new(total);
    for (index, id) in [1, 2].into_iter().enumerate() {
        let result = journal
            .lookup_observed_with_budget(&fixture.source, &[operation_id(id)], &mut budget)
            .unwrap();
        assert_eq!(
            result.operations[&operation_id(id)],
            evidence(id, &[id - 1])
        );
        assert_eq!(
            budget.consumed(),
            (index + 1) * state + payloads[..=index].iter().sum::<usize>()
        );
    }
    assert_eq!(budget.remaining(), 0);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[], &mut budget),
        "byte limit",
    );
    assert_eq!(budget.consumed(), total);
}

#[test]
fn jj_journal_budget_is_aggregate_within_a_lookup_and_does_not_return_partial_evidence() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    capture_two(&fixture, &mut journal);
    let (state, payloads) = stored_lengths(&fixture, &[1, 2]);
    let mut budget = ReadBudget::new(state + payloads[0]);
    rejected(
        journal.lookup_observed_with_budget(
            &fixture.source,
            &[operation_id(2), operation_id(1)],
            &mut budget,
        ),
        "byte limit",
    );
    assert_eq!(budget.consumed(), state + payloads[0]);
    assert_eq!(budget.remaining(), 0);
    assert_eq!(
        journal.status(&fixture.source).unwrap().pending_operations,
        2
    );
}

#[test]
fn jj_journal_budget_is_not_refunded_after_a_later_checksum_failure() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    capture_two(&fixture, &mut journal);
    replace_payload(&fixture, 2, b"broken", "0");
    let (state, payloads) = stored_lengths(&fixture, &[1, 2]);
    let total = 2 * state + payloads.iter().sum::<usize>();
    let mut budget = ReadBudget::new(total);
    journal
        .lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget)
        .unwrap();
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(2)], &mut budget),
        "checksum",
    );
    assert_eq!(budget.consumed(), total);
    assert_eq!(budget.remaining(), 0);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[], &mut budget),
        "byte limit",
    );
    assert_eq!(budget.consumed(), total);
}

#[test]
fn jj_journal_budget_charges_a_corrupt_state_blob_without_reading_operations() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_sources SET state = ?1 WHERE source_id = ?2",
        params![b"broken state".as_slice(), fixture.source],
    )
    .unwrap();
    let (state, payloads) = stored_lengths(&fixture, &[1]);
    let mut budget = ReadBudget::new(state + payloads[0]);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
        "checksum",
    );
    assert_eq!(budget.consumed(), state);
    assert_eq!(budget.remaining(), payloads[0]);
}

#[test]
fn jj_journal_budget_charges_checksums_valid_but_undecodable_payloads() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let invalid = b"\xff";
    replace_payload(
        &fixture,
        1,
        invalid,
        &format!("{:x}", Sha256::digest(invalid)),
    );
    let (state, payloads) = stored_lengths(&fixture, &[1]);
    let total = state + payloads[0];
    let mut budget = ReadBudget::new(total);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
        "decode",
    );
    assert_eq!(budget.consumed(), total);
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn jj_journal_budget_charges_selected_payload_before_rejecting_sequence_gap() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET sequence = 2 WHERE source_id = ?1",
        [&fixture.source],
    )
    .unwrap();
    let (state, payloads) = stored_lengths(&fixture, &[1]);
    let total = state + payloads[0];
    let mut budget = ReadBudget::new(total);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
        "gap",
    );
    assert_eq!(budget.consumed(), total);
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn jj_journal_budget_rejects_large_wrong_type_sequence_after_payload_charge() {
    for blob_sequence in [true, false] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        fixture.capture_first(&mut journal);
        let (state, payloads) = stored_lengths(&fixture, &[1]);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        if blob_sequence {
            conn.execute(
                "UPDATE jj_operations SET sequence = zeroblob(?1) WHERE source_id = ?2",
                params![256 * 1024, fixture.source],
            )
            .unwrap();
        } else {
            conn.execute(
                "UPDATE jj_operations SET sequence = ?1 WHERE source_id = ?2",
                params!["é".repeat(128 * 1024), fixture.source],
            )
            .unwrap();
        }
        let total = state + payloads[0];
        let mut budget = ReadBudget::new(total);
        rejected(
            journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
            "sequence",
        );
        assert_eq!(budget.consumed(), total);
        assert_eq!(budget.remaining(), 0);
    }
}

#[test]
fn jj_journal_budget_charges_selected_blobs_when_checksum_metadata_has_wrong_type() {
    for source_checksum in [true, false] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        fixture.capture_first(&mut journal);
        let (state, payloads) = stored_lengths(&fixture, &[1]);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let sql = if source_checksum {
            "UPDATE jj_sources SET checksum = zeroblob(65) WHERE source_id = ?1"
        } else {
            "UPDATE jj_operations SET checksum = zeroblob(65) WHERE source_id = ?1"
        };
        conn.execute(sql, [&fixture.source]).unwrap();
        let mut budget = ReadBudget::new(state + payloads[0]);
        assert!(
            journal
                .lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget,)
                .is_err()
        );
        let selected = if source_checksum {
            state
        } else {
            state + payloads[0]
        };
        assert_eq!(budget.consumed(), selected);
        assert_eq!(budget.remaining(), state + payloads[0] - selected);
    }
}

#[test]
fn jj_journal_budget_rejects_text_in_blob_columns_without_character_byte_confusion() {
    for source_payload in [true, false] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        fixture.capture_first(&mut journal);
        let (state, _) = stored_lengths(&fixture, &[]);
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let sql = if source_payload {
            "UPDATE jj_sources SET state = ?1 WHERE source_id = ?2"
        } else {
            "UPDATE jj_operations SET payload = ?1 WHERE source_id = ?2"
        };
        conn.execute(sql, params!["é".repeat(2048), fixture.source])
            .unwrap();
        let selected_state = if source_payload { 0 } else { state };
        let mut budget = ReadBudget::new(selected_state + 2048);
        assert!(
            journal
                .lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget,)
                .is_err()
        );
        assert_eq!(budget.consumed(), selected_state);
        assert_eq!(budget.remaining(), 2048);
    }
}

#[test]
fn jj_journal_budget_rejects_oversized_state_without_materializing_its_blob() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_sources SET state = zeroblob(?1) WHERE source_id = ?2",
        params![128 * 1024 + 1, fixture.source],
    )
    .unwrap();
    let mut budget = ReadBudget::new(usize::MAX);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[], &mut budget),
        "byte limit",
    );
    assert_eq!(budget.consumed(), 0);
    assert_eq!(budget.remaining(), usize::MAX);
}

#[test]
fn jj_journal_budget_preserves_per_record_cap_even_with_a_large_caller_budget() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_operations SET payload = zeroblob(?1) WHERE source_id = ?2",
        params![
            2 * MAX_JJ_OBSERVATION_OPERATION_BYTES + 64 * 1024 + 1,
            fixture.source
        ],
    )
    .unwrap();
    let (state, _) = stored_lengths(&fixture, &[]);
    let mut budget = ReadBudget::new(usize::MAX);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
        "byte limit",
    );
    assert_eq!(budget.consumed(), state);
    assert_eq!(budget.remaining(), usize::MAX - state);
}

#[test]
fn jj_journal_budget_preserves_raw_evidence_cap_after_encoded_blob_selection() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let mut oversized = evidence(1, &[0]);
    oversized.operation_bytes = vec![0; MAX_JJ_OBSERVATION_OPERATION_BYTES + 1];
    oversized.view_bytes.clear();
    let stored = serde_json::json!({
        "schema_version": JJ_OBSERVATION_SCHEMA_VERSION,
        "reader_profile": JJ_OBSERVATION_READER_PROFILE,
        "source_id": fixture.source,
        "evidence": oversized,
    });
    let mut payload = Vec::new();
    ciborium::into_writer(&stored, &mut payload).unwrap();
    replace_payload(
        &fixture,
        1,
        &payload,
        &format!("{:x}", Sha256::digest(&payload)),
    );
    let (state, lengths) = stored_lengths(&fixture, &[1]);
    let total = state + lengths[0];
    let mut budget = ReadBudget::new(total);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
        "byte limit",
    );
    assert_eq!(budget.consumed(), total);
    assert_eq!(budget.remaining(), 0);
}

#[test]
fn jj_journal_budget_preserves_independent_eight_mib_per_call_cap() {
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
    let (state, payloads) = stored_lengths(&fixture, &[1, 2, 3, 4]);
    assert!(payloads[..3].iter().sum::<usize>() < MAX_JJ_OBSERVATION_BATCH_BYTES);
    assert!(payloads.iter().sum::<usize>() > MAX_JJ_OBSERVATION_BATCH_BYTES);
    let mut budget = ReadBudget::new(usize::MAX);
    rejected(
        journal.lookup_observed_with_budget(
            &fixture.source,
            &[
                operation_id(1),
                operation_id(2),
                operation_id(3),
                operation_id(4),
            ],
            &mut budget,
        ),
        "byte limit",
    );
    assert_eq!(
        budget.consumed(),
        state + payloads[..3].iter().sum::<usize>()
    );
}

#[test]
fn jj_journal_budget_validates_requests_without_consuming_payload_budget() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    for ids in [
        vec![operation_id(0)],
        vec![operation_id(1), operation_id(1)],
        vec!["invalid".to_owned()],
        (1..=MAX_JJ_OBSERVATION_LOOKUP_LIMIT as u64 + 1)
            .map(operation_id)
            .collect(),
    ] {
        let mut budget = ReadBudget::new(usize::MAX);
        assert!(
            journal
                .lookup_observed_with_budget(&fixture.source, &ids, &mut budget)
                .is_err()
        );
        assert_eq!(budget.consumed(), 0);
    }
    let mut budget = ReadBudget::new(usize::MAX);
    rejected(
        journal.lookup_observed_with_budget("bad source", &[], &mut budget),
        "source",
    );
    assert_eq!(budget.consumed(), 0);
}

#[test]
fn jj_journal_budget_keeps_source_isolation_and_missing_state_checks() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
    let mut budget = ReadBudget::new(0);
    let other = journal
        .lookup_observed_with_budget(&source_id(2), &[operation_id(1)], &mut budget)
        .unwrap();
    assert!(other.operations.is_empty());
    assert_eq!(budget.consumed(), 0);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", false).unwrap();
    conn.execute(
        "DELETE FROM jj_sources WHERE source_id = ?1",
        [&fixture.source],
    )
    .unwrap();
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[], &mut budget),
        "gap",
    );
    assert_eq!(budget.consumed(), 0);
}

#[test]
fn jj_journal_budget_keeps_requested_missing_head_failure_after_state_charge() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    fixture.capture_first(&mut journal);
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
    let (state, _) = stored_lengths(&fixture, &[]);
    let mut budget = ReadBudget::new(state);
    rejected(
        journal.lookup_observed_with_budget(&fixture.source, &[operation_id(1)], &mut budget),
        "gap",
    );
    assert_eq!(budget.consumed(), state);
    assert_eq!(budget.remaining(), 0);
}
