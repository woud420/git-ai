use super::*;

#[test]
fn registration_complete_reads_share_exact_inclusive_and_repeated_byte_budget() {
    let fixture = Fixture::complete();
    let total = charged_lengths(&fixture.journal.conn).iter().sum::<usize>();
    let mut budget = ReadBudget::new(2 * total);
    for expected in [total, 2 * total] {
        assert!(read(&fixture, "default", &mut budget).unwrap().is_some());
        assert_eq!(budget.consumed(), expected);
    }
    assert!(!error_text(read(&fixture, "default", &mut budget)).is_empty());
    assert_eq!(budget.consumed(), 2 * total);
}

#[test]
fn registration_complete_read_short_budget_keeps_every_prior_selected_charge() {
    let fixture = Fixture::complete();
    let lengths = charged_lengths(&fixture.journal.conn);
    let total = lengths.iter().sum::<usize>();
    let mut budget = ReadBudget::new(total - 1);
    assert!(!error_text(read(&fixture, "default", &mut budget)).is_empty());
    assert_eq!(budget.consumed(), lengths[..3].iter().sum::<usize>());
    assert_eq!(budget.remaining(), lengths[3] - 1);
}

#[test]
fn registration_complete_read_gates_sql_payload_and_scalar_types_before_allocation() {
    for update in [
        "UPDATE jj_native_workspaces SET record=zeroblob(131073)",
        "UPDATE jj_native_workspaces SET record=42",
        "UPDATE jj_native_workspaces SET checksum=zeroblob(1000000)",
        "UPDATE jj_native_workspaces SET locator_key=zeroblob(1000000)",
    ] {
        let fixture = Fixture::complete();
        fixture.journal.conn.execute(update, []).unwrap();
        let mut budget = ReadBudget::new(LIMIT);
        assert!(
            !error_text(read(&fixture, "default", &mut budget)).is_empty(),
            "{update}"
        );
        let expected = if update.contains("SET record") {
            SOURCE_RAW.len()
        } else {
            SOURCE_RAW.len() + WORKSPACE_RAW.len()
        };
        assert_eq!(budget.consumed(), expected, "{update}");
    }
}

#[test]
fn registration_staged_readback_uses_the_callers_budget_and_rolls_back_on_exhaustion() {
    let control = Fixture::complete();
    let lengths = charged_lengths(&control.journal.conn);
    let total = lengths.iter().sum::<usize>();
    let mut fixture = Fixture::new(false);
    let before = snapshot(&fixture.journal.conn);
    let mut budget = ReadBudget::new(2 * total - 1);
    assert!(
        control
            .journal
            .read_registration_snapshot(SOURCE, "default", &mut budget)
            .unwrap()
            .is_some()
    );
    let prepared = super::support::prepared(&fixture.seed);
    assert!(
        !error_text(
            fixture
                .journal
                .stage_registration_install(prepared, &mut budget)
        )
        .is_empty()
    );
    assert_eq!(
        budget.consumed(),
        total + lengths[..3].iter().sum::<usize>()
    );
    assert_eq!(snapshot(&fixture.journal.conn), before);

    let prepared = super::support::prepared(&fixture.seed);
    let mut exact = ReadBudget::new(total);
    let staged = fixture
        .journal
        .stage_registration_install(prepared, &mut exact)
        .unwrap();
    assert_eq!(exact.consumed(), total);
    staged.commit().unwrap();
    assert_eq!(native_counts(&fixture.journal.conn), [1; 4]);
}
