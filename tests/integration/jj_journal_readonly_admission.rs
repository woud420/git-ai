use super::*;
use git_ai::operations::jj::admission::read_native_admission;
use std::time::Instant;

#[allow(dead_code)]
#[path = "fixtures/jj-admission/vectors.rs"]
mod vectors;

#[test]
fn jj_journal_readonly_reopens_a_native_admission_from_saved_sql_bytes() {
    let fixture = manual_v4(ADMISSIONS, STATES);
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.pragma_update(None, "foreign_keys", true).unwrap();
    let packet = vectors::RECORDS.iter().find(|v| v.label == "left").unwrap();
    let state = vectors::RECORDS
        .iter()
        .find(|v| v.label == "left_state")
        .unwrap();
    conn.execute(
        "INSERT INTO jj_native_admissions VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            vectors::SOURCE,
            packet.admission_id,
            packet.generation,
            packet.raw,
            packet.checksum
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO jj_native_admission_states VALUES (?1, ?2, ?3, ?4)",
        params![
            vectors::SOURCE,
            state.admission_id,
            state.raw,
            state.checksum
        ],
    )
    .unwrap();
    let before = readonly_snapshot(&conn);
    for _ in 0..2 {
        let journal = JjObservationJournal::open_read_only_at_path(&fixture.path).unwrap();
        let admission = read_native_admission(
            &journal,
            &fixture.source,
            packet.admission_id,
            Instant::now() + Duration::from_secs(10),
            &mut ReadBudget::new(32 * 1024 * 1024),
        )
        .unwrap()
        .unwrap();
        assert_eq!(admission.receipt().admission_id(), packet.admission_id);
        assert_eq!(admission.receipt().generation(), 1);
        assert_eq!(admission.receipt().baseline_id(), vectors::BASELINE);
        assert_eq!(admission.ordered_operations().len(), 1);
        assert_eq!(admission.reached_baseline_ids(), [vectors::FIRST]);
        assert!(!admission.reaches_root());
    }
    assert!(
        readonly_snapshot(&conn) == before,
        "historical read changed journal data"
    );
}
