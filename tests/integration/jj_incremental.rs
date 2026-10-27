use super::*;
use git_ai::operations::jj::observation::{JjObservationSession, JjObservationStep};

#[path = "jj_incremental_faults.rs"]
mod faults;

#[test]
fn jj_incremental_admits_beyond_256_and_replays_after_native_history_gc() {
    for colocated in [true, false] {
        run_case(
            "history:admission:incremental:chain",
            colocated,
            Policy::Root,
        );
    }
}

pub(super) fn run(case: &Case, config: &Config) {
    if !case.name.ends_with(":chain") {
        return faults::run(case, config);
    }
    let (journal, saved, _) = initial(case, config);
    let mut session = JjObservationSession::new(
        journal,
        case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    assert!(matches!(
        session
            .step(config, deadline(), &mut admission_budget())
            .unwrap(),
        JjObservationStep::Unchanged
    ));
    let records = native::chain(257);
    write_records(case, &records);
    for (head, expected_local_count) in [(127, 128), (256, 129)] {
        case.heads(&[&records[head].operation_id]);
        let JjObservationStep::Admitted(segment) = session
            .step(config, deadline(), &mut admission_budget())
            .unwrap()
        else {
            panic!("changed integrated heads did not produce a segment")
        };
        assert_eq!(segment.local_operations().len(), expected_local_count);
        assert_eq!(
            segment.original_baseline_id(),
            saved.baseline().receipt().baseline_id()
        );
        assert_eq!(
            segment.head_closures()[0].reached_baseline_ids(),
            [MERGE_ID]
        );
    }
    let cursor = session.verified_cursor().unwrap().clone();
    let stored = session
        .read_latest_segment(config, deadline(), &mut admission_budget())
        .unwrap()
        .unwrap();
    assert_eq!(stored.local_operations(), &records[128..]);
    assert_eq!(cursor.generation(), 2);
    drop(session);
    for record in &records[..256] {
        fs::remove_file(op_path(case, &record.operation_id)).unwrap();
    }
    let mut resumed = JjObservationSession::new(
        case.open(),
        case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    assert!(resumed.verified_cursor().is_none());
    let mut jobs = 0;
    loop {
        jobs += 1;
        assert!(jobs <= 8);
        match resumed
            .replay_step(config, deadline(), &mut admission_budget())
            .unwrap()
        {
            JjObservationStep::Replaying => assert!(resumed.verified_cursor().is_none()),
            JjObservationStep::Unchanged => break,
            JjObservationStep::Admitted(_) => panic!("restart duplicated the saved observation"),
        }
    }
    assert_eq!(resumed.verified_cursor(), Some(&cursor));
    let state = case
        .sql()
        .query_row("SELECT count(*) FROM jj_native_admissions", [], |row| {
            row.get::<_, usize>(0)
        })
        .unwrap();
    assert_eq!(state, 2);
    assert!(
        case.open()
            .status(saved.source_id())
            .unwrap()
            .applied_heads
            .is_empty()
    );
}
