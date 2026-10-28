use super::*;

#[test]
fn jj_incremental_cache_rejects_triggers_external_progress_short_budget_and_moved_database() {
    for kind in ["trigger", "external", "budget", "moved"] {
        run_case(
            &format!("history:admission:incremental:{kind}"),
            true,
            Policy::Root,
        );
    }
}

#[test]
fn jj_incremental_missing_dependency_requires_recovery_without_rebaselining() {
    run_case("history:admission:incremental:missing", false, Policy::Root);
}

#[test]
fn jj_incremental_restart_replays_long_chains_in_bounded_jobs_without_held_snapshots() {
    run_case("history:admission:incremental:long", true, Policy::Root);
}

fn open(case: &Case, config: &Config) -> JjObservationSession {
    JjObservationSession::new(
        case.open(),
        case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap()
}

fn admitted(
    case: &Case,
    config: &Config,
    session: &mut JjObservationSession,
    record: &JjOperationEvidence,
) {
    select(
        case,
        std::slice::from_ref(record),
        &[&record.operation_id],
        None,
    );
    assert!(matches!(
        session
            .step(config, deadline(), &mut admission_budget())
            .unwrap(),
        JjObservationStep::Admitted(_)
    ));
}

fn replay(
    case: &Case,
    config: &Config,
    session: &mut JjObservationSession,
    maximum: usize,
) -> usize {
    for job in 1..=maximum {
        let conn = case.sql();
        conn.busy_timeout(Duration::ZERO).unwrap();
        conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK").unwrap();
        match session
            .replay_step(config, deadline(), &mut admission_budget())
            .unwrap()
        {
            JjObservationStep::Replaying => assert!(session.verified_cursor().is_none()),
            JjObservationStep::Unchanged => return job,
            JjObservationStep::Admitted(_) => panic!("read-only replay wrote an admission"),
        }
    }
    panic!("replay did not finish within the fixture bound")
}

pub(super) fn run(case: &Case, config: &Config) {
    let (journal, saved, _) = initial(case, config);
    let mut session = JjObservationSession::new(
        journal,
        case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    )
    .unwrap();
    let a = native::rich_parent();
    let b = native::rich_child();
    admitted(case, config, &mut session, &a);
    let kind = case.name.rsplit(':').next().unwrap();
    if kind == "long" {
        for generation in 2..=70 {
            if generation % 2 == 0 {
                case.heads(&[MERGE_ID]);
            } else {
                case.heads(&[&a.operation_id]);
            }
            assert!(matches!(
                session
                    .step(config, deadline(), &mut admission_budget())
                    .unwrap(),
                JjObservationStep::Admitted(_)
            ));
        }
        let cursor = session.verified_cursor().unwrap().clone();
        drop(session);
        let mut session = open(case, config);
        let jobs = replay(case, config, &mut session, 150);
        assert!(
            jobs > 64,
            "restart must not treat a generation number as proof"
        );
        assert_eq!(session.verified_cursor(), Some(&cursor));
        let latest = session
            .read_latest_segment(config, deadline(), &mut admission_budget())
            .unwrap()
            .unwrap();
        assert_eq!(latest.receipt().generation(), 70);
        assert!(latest.local_operations().is_empty());
        return;
    }
    if kind == "missing" {
        admitted(case, config, &mut session, &b);
        drop(session);
        let conn = case.sql();
        let row: (String, Vec<u8>, String) = conn
            .query_row(
                "SELECT admission_id,record,checksum FROM jj_native_admissions WHERE generation=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        conn.execute("DELETE FROM jj_native_admissions WHERE generation=1", [])
            .unwrap();
        let mut session = open(case, config);
        assert!(matches!(
            session
                .replay_step(config, deadline(), &mut admission_budget())
                .unwrap(),
            JjObservationStep::Replaying
        ));
        assert!(
            session
                .replay_step(config, deadline(), &mut admission_budget())
                .is_err()
        );
        assert!(session.verified_cursor().is_none());
        conn.execute("INSERT INTO jj_native_admissions(source_id,admission_id,generation,record,checksum) VALUES (?1,?2,1,?3,?4)", rusqlite::params![saved.source_id(),row.0,row.1,row.2]).unwrap();
        drop(session);
        let mut session = open(case, config);
        replay(case, config, &mut session, 8);
        assert_eq!(session.verified_cursor().unwrap().generation(), 2);
        assert_eq!(
            session.verified_cursor().unwrap().baseline_id(),
            saved.baseline().receipt().baseline_id()
        );
        return;
    }
    if kind == "external" {
        let mut writer = case.open();
        let current = session.verified_cursor().unwrap().clone();
        outcome(admit_now(case, &mut writer, config, &current), false);
        assert!(
            session
                .step(config, deadline(), &mut admission_budget())
                .is_err()
        );
        assert!(session.verified_cursor().is_none());
        drop(session);
        let mut session = open(case, config);
        replay(case, config, &mut session, 4);
        assert_eq!(session.verified_cursor().unwrap().generation(), 2);
        return;
    }
    select(case, std::slice::from_ref(&b), &[&b.operation_id], None);
    let before = admission_rows(case);
    if kind == "budget" {
        assert!(
            session
                .step(config, deadline(), &mut ReadBudget::new(1))
                .is_err()
        );
        assert!(session.verified_cursor().is_none());
        assert_eq!(admission_rows(case), before);
    } else if kind == "moved" {
        let moved = case.journal_path.with_extension("moved");
        fs::rename(&case.journal_path, &moved).unwrap();
        let result = session.step(config, deadline(), &mut admission_budget());
        fs::rename(moved, &case.journal_path).unwrap();
        assert!(result.is_err());
        assert!(session.verified_cursor().is_none());
        assert_eq!(admission_rows(case), before);
    } else if kind == "trigger" {
        case.sql().execute_batch("CREATE TRIGGER corrupt_prefix BEFORE INSERT ON jj_native_admissions BEGIN UPDATE jj_native_admissions SET checksum='corrupted'; END").unwrap();
        assert!(matches!(
            session
                .step(config, deadline(), &mut admission_budget())
                .unwrap(),
            JjObservationStep::Replaying
        ));
        let error = session
            .step(config, deadline(), &mut admission_budget())
            .unwrap_err();
        assert!(error.to_string().contains("refuses triggers"), "{error}");
        assert!(session.verified_cursor().is_none());
        assert_eq!(admission_rows(case), before);
        case.sql()
            .execute_batch("DROP TRIGGER corrupt_prefix")
            .unwrap();
    } else {
        panic!("unknown incremental fixture {kind}");
    }
    assert!(matches!(
        session
            .step(config, deadline(), &mut admission_budget())
            .unwrap(),
        JjObservationStep::Admitted(_)
    ));
    assert_eq!(session.verified_cursor().unwrap().generation(), 2);
}
