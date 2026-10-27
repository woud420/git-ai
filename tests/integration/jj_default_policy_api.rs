use super::*;

pub(super) fn run(case: &Case, config: &Config, alternating: bool) {
    let (mut journal, saved, zero) = initial(case, config);
    let records = records();
    write_records(case, &records);
    let mut expected = zero.clone();
    let mut first = None;
    for generation in 1..=12 {
        let both = alternating && generation % 2 == 0;
        select(case, &[], &heads(&records, both), None);
        let mut reads = if generation == 9 {
            let required = exact_write_bytes(case, &saved, &expected, &records, both);
            let before = total_state(case);
            let mut short = ReadBudget::new(required - 1);
            let error = failed(write_attempt(
                case,
                &mut journal,
                config,
                &saved,
                &expected,
                alternating,
                &mut short,
            ));
            assert!(matches!(error, JjNativeAdmissionError::Journal(_)));
            assert!(error.to_string().contains("read byte limit"));
            assert!(
                total_state(case) == before,
                "short-budget attempt escaped rollback"
            );
            assert!(short.consumed() > 0 && short.consumed() < required);
            ReadBudget::new(required)
        } else {
            admission_budget()
        };
        let value = outcome(
            write_attempt(
                case,
                &mut journal,
                config,
                &saved,
                &expected,
                alternating,
                &mut reads,
            )
            .unwrap(),
            false,
        );
        if generation == 9 {
            assert_eq!(
                reads.remaining(),
                0,
                "changed default path selected unexpected payloads"
            );
        }
        let raw = assert_packet(case, value.admission(), &saved, &expected, &records, both);
        assert_cursor(
            value.current_cursor(),
            &saved,
            generation,
            &heads(&records, both),
        );
        same_receipt(value.registration(), &saved);
        require_rows(case, generation as usize);
        if generation == 1 {
            first = Some((value.admission().receipt().admission_id().to_owned(), raw));
        }
        let before = total_state(case);
        let retry = outcome(
            write_attempt(
                case,
                &mut journal,
                config,
                &saved,
                &expected,
                alternating,
                &mut admission_budget(),
            )
            .unwrap(),
            true,
        );
        assert_eq!(
            retry.admission().receipt().admission_id(),
            value.admission().receipt().admission_id()
        );
        assert_packet(case, retry.admission(), &saved, &expected, &records, both);
        assert_eq!(retry.current_cursor(), value.current_cursor());
        assert!(total_state(case) == before);
        expected = value.current_cursor().clone();
        if alternating {
            let unchanged = reconcile_registered_history(
                &mut journal,
                &case.context(),
                config,
                NativeReconciliationExpectation {
                    admission: expected.expectation(),
                    workspace_name: saved.workspace_name(),
                    attachment_id: saved.attachment_id(),
                },
                deadline(),
                &mut admission_budget(),
            )
            .unwrap();
            let NativeReconciliationOutcome::Unchanged(unchanged) = unchanged else {
                panic!("unchanged reconciliation appended another packet")
            };
            assert_eq!(unchanged.cursor(), &expected);
            same_receipt(unchanged.registration(), &saved);
            assert!(total_state(case) == before);
        }
        if generation == 8 {
            drop(journal);
            journal = case.open();
            assert_eq!(status(case, &journal, config).cursor(), &expected);
        }
    }
    let (first_id, original) = first.unwrap();
    select(case, &[], &heads(&records, false), None);
    let before = total_state(case);
    for _ in 0..2 {
        let retry = outcome(
            write_attempt(
                case,
                &mut journal,
                config,
                &saved,
                &zero,
                alternating,
                &mut admission_budget(),
            )
            .unwrap(),
            true,
        );
        assert_eq!(retry.admission().receipt().admission_id(), first_id);
        assert_eq!(retry.admission().receipt().generation(), 1);
        assert_eq!(retry.current_cursor(), &expected);
        assert_eq!(
            assert_packet(case, retry.admission(), &saved, &zero, &records, false),
            original
        );
    }
    let historical = packet(case, &journal, saved.source_id(), &first_id);
    assert_eq!(
        assert_packet(case, &historical, &saved, &zero, &records, false),
        original
    );
    assert!(total_state(case) == before);
    require_rows(case, 12);
}
