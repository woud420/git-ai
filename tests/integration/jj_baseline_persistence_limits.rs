use super::*;

#[test]
fn jj_baseline_persistence_exact_payload_budget_is_inclusive_and_shared_across_reopens() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let bytes = stored_bytes(&fixture);
    let mut budget = ReadBudget::new(2 * bytes);
    for _ in 0..2 {
        let durable = reopen_current_state_baseline(&journal, &fixture.source, &mut budget)
            .unwrap()
            .unwrap();
        assert_eq!(durable.anchors(), [first()]);
    }
    assert_eq!(budget.remaining(), 0);
    assert_eq!(budget.consumed(), 2 * bytes);
    rejected(
        reopen_current_state_baseline(&journal, &fixture.source, &mut budget),
        "limit",
    );
    assert_eq!(budget.consumed(), 2 * bytes);
}

#[test]
fn jj_baseline_persistence_insufficient_budget_retains_only_selected_state_charge() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    let state_bytes: usize = conn
        .query_row("SELECT length(state) FROM jj_native_sources", [], |row| {
            row.get(0)
        })
        .unwrap();
    for (allowance, charged) in [
        (0, 0),
        (state_bytes - 1, 0),
        (state_bytes, state_bytes),
        (stored_bytes(&fixture) - 1, state_bytes),
    ] {
        let mut budget = ReadBudget::new(allowance);
        rejected(
            reopen_current_state_baseline(&journal, &fixture.source, &mut budget),
            "limit",
        );
        assert_eq!(budget.consumed(), charged);
    }
}

#[test]
fn jj_baseline_persistence_oversized_sql_payloads_are_not_selected() {
    for state in [true, false] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let state_bytes: usize = conn
            .query_row("SELECT length(state) FROM jj_native_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        let sql = if state {
            "UPDATE jj_native_sources SET state = zeroblob(131073)"
        } else {
            "UPDATE jj_native_baselines SET record = zeroblob(8388609)"
        };
        conn.execute(sql, []).unwrap();
        let before = native_rows(&fixture);
        let mut budget = ReadBudget::new(32 * 1024 * 1024);
        rejected(
            reopen_current_state_baseline(&journal, &fixture.source, &mut budget),
            "limit",
        );
        assert_eq!(budget.consumed(), if state { 0 } else { state_bytes });
        assert_eq!(native_rows(&fixture), before);
    }
}

#[test]
fn jj_baseline_persistence_wrong_sql_payload_types_are_not_materialized_as_blobs() {
    for state in [true, false] {
        let fixture = Fixture::new();
        let mut journal = fixture.open();
        installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
        let conn = open_with_memory_limits(&fixture.path).unwrap();
        let state_bytes: usize = conn
            .query_row("SELECT length(state) FROM jj_native_sources", [], |row| {
                row.get(0)
            })
            .unwrap();
        let sql = if state {
            "UPDATE jj_native_sources SET state = CAST(zeroblob(1048576) AS TEXT)"
        } else {
            "UPDATE jj_native_baselines SET record = CAST(zeroblob(1048576) AS TEXT)"
        };
        conn.execute(sql, []).unwrap();
        let mut budget = full_budget();
        rejected(
            reopen_current_state_baseline(&journal, &fixture.source, &mut budget),
            "blob",
        );
        assert_eq!(budget.consumed(), if state { 0 } else { state_bytes });
    }
}

#[test]
fn jj_baseline_persistence_sibling_source_corruption_does_not_consume_our_budget() {
    let fixture = Fixture::new();
    let mut journal = fixture.open();
    installed(install(&mut journal, &fixture.source, &[first()]).unwrap());
    let sibling = "02".repeat(32);
    installed(install(&mut journal, &sibling, &[left()]).unwrap());
    let conn = open_with_memory_limits(&fixture.path).unwrap();
    conn.execute(
        "UPDATE jj_native_sources SET state = zeroblob(1048576) WHERE source_id = ?1",
        [&sibling],
    )
    .unwrap();
    conn.execute(
        "UPDATE jj_native_baselines SET record = zeroblob(9437184) WHERE source_id = ?1",
        [&sibling],
    )
    .unwrap();
    let bytes = stored_bytes(&fixture);
    let mut budget = ReadBudget::new(bytes);
    let durable = reopen_current_state_baseline(&journal, &fixture.source, &mut budget)
        .unwrap()
        .unwrap();
    assert_eq!(durable.anchors(), [first()]);
    assert_eq!(budget.consumed(), bytes);
}
