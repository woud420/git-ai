use super::super::super::super::support::collect as collect_history;
use super::*;
use crate::jj_ancestry::head_closures::{assert_closures, summary};

#[test]
fn jj_head_closures_collected_history_keeps_terminal_mixed_and_root_heads_distinct() {
    for colocated in [true, false] {
        cli_case("write:head_closures:history_mixed", colocated, Policy::Root);
    }
}

#[test]
fn jj_head_closures_repeated_history_collection_preserves_shared_order_and_storage() {
    cli_case("write:head_closures:history_repeated", true, Policy::Root);
}

#[test]
fn jj_head_closures_admission_write_reopen_and_historical_retry_preserve_v1_storage() {
    cli_case("write:head_closures:admission", true, Policy::Root);
}

#[test]
fn jj_head_closures_debug_capture_and_receipt_emit_exact_derived_metadata() {
    cli_case("write:head_closures:json", false, Policy::Root);
}

pub(super) fn dispatch(case: &Case, config: &Config) {
    match case.name.rsplit(':').next().unwrap() {
        "history_mixed" => history_mixed(case, config),
        "history_repeated" => history_repeated(case, config),
        "admission" => admission(case, config),
        "json" => cli_json(case, config),
        other => panic!("unknown per-head fixture {other}"),
    }
}

fn history_mixed(case: &Case, config: &Config) {
    let (journal, saved, _) = initial(case, config);
    let records = [
        native::first(),
        native::left(),
        native::late_branch(),
        native::rich_parent(),
        native::mixed_merge(),
    ];
    let heads = [
        MERGE_ID,
        native::RICH_PARENT_ID,
        native::LATE_BRANCH_ID,
        native::MIXED_MERGE_ID,
    ];
    select(case, &records, &heads, None);
    let before = total_state(case);
    let result = collect_history(case, &journal, config);
    assert_closures(
        result.head_closures(),
        &[
            (MERGE_ID, &[MERGE_ID], false),
            (native::RICH_PARENT_ID, &[MERGE_ID], false),
            (native::LATE_BRANCH_ID, &[], true),
            (native::MIXED_MERGE_ID, &[MERGE_ID], true),
        ],
    );
    assert_eq!(result.reached_baseline_ids(), [MERGE_ID]);
    assert!(result.reaches_root());
    assert_eq!(result.ordered_operations().len(), records.len());
    for record in result.ordered_operations() {
        assert_eq!(
            record,
            records
                .iter()
                .find(|raw| raw.operation_id == record.operation_id)
                .unwrap()
        );
    }
    same_receipt(result.registration(), &saved);
    assert!(total_state(case) == before);
    assert_eq!(status(case, &journal, config).cursor().generation(), 0);
}

fn history_repeated(case: &Case, config: &Config) {
    let (journal, saved, _) = initial(case, config);
    let records = [
        native::rich_parent(),
        native::rich_child(),
        native::converged_merge(),
    ];
    let heads = [native::CONVERGED_MERGE_ID, MERGE_ID, native::RICH_PARENT_ID];
    select(case, &records, &heads, None);
    let before = total_state(case);
    let first = collect_history(case, &journal, config);
    assert_closures(
        first.head_closures(),
        &[
            (native::CONVERGED_MERGE_ID, &[MERGE_ID], false),
            (MERGE_ID, &[MERGE_ID], false),
            (native::RICH_PARENT_ID, &[MERGE_ID], false),
        ],
    );
    assert_eq!(first.ordered_operations(), records);
    let reverse = heads.iter().rev().copied().collect::<Vec<_>>();
    case.heads(&reverse);
    for _ in 0..2 {
        let repeated = collect_history(case, &journal, config);
        assert_eq!(repeated.head_closures(), first.head_closures());
        assert_eq!(repeated.ordered_operations(), first.ordered_operations());
        assert_eq!(repeated.head_ids(), first.head_ids());
        same_receipt(repeated.registration(), &saved);
    }
    assert!(total_state(case) == before);
}

fn select_disjoint(case: &Case) {
    select(
        case,
        &[
            native::first(),
            native::left(),
            native::late_branch(),
            native::rich_parent(),
        ],
        &[native::LATE_BRANCH_ID, native::RICH_PARENT_ID],
        None,
    );
}

fn assert_disjoint(value: &DurableNativeAdmission) {
    assert_closures(
        value.head_closures(),
        &[
            (native::LATE_BRANCH_ID, &[], true),
            (native::RICH_PARENT_ID, &[MERGE_ID], false),
        ],
    );
    assert_eq!(value.reached_baseline_ids(), [MERGE_ID]);
    assert!(value.reaches_root());
    assert_eq!(value.ordered_operations().len(), 4);
}

fn stored_packet(case: &Case, source: &str, id: &str) -> (Vec<u8>, String, u64) {
    let value: (Vec<u8>, String, u64) = case.sql().query_row(
        "SELECT record,checksum,generation FROM jj_native_admissions WHERE source_id=?1 AND admission_id=?2",
        [source, id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert!(value.0.len() <= 8 * 1024 * 1024);
    assert_eq!(value.1, id);
    let decoded: ciborium::Value = ciborium::from_reader(value.0.as_slice()).unwrap();
    let fields = decoded.as_map().unwrap();
    let names = fields
        .iter()
        .map(|(name, _)| name.as_text().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "record_version",
            "domain",
            "source_id",
            "reader_profile",
            "initialization_receipt_id",
            "baseline_id",
            "baseline_generation",
            "expected_admission_generation",
            "expected_admitted_head_ids",
            "captured_head_ids",
            "operations",
        ]
    );
    assert_eq!(fields[0].1, ciborium::Value::Integer(1.into()));
    value
}

fn admission(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    select_disjoint(case);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    assert_disjoint(first.admission());
    let id = first.admission().receipt().admission_id();
    let original = stored_packet(case, saved.source_id(), id);
    assert_eq!(original.2, 1);
    select(
        case,
        &[native::rich_child()],
        &[native::RICH_CHILD_ID],
        None,
    );
    let later = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    assert_closures(
        later.admission().head_closures(),
        &[(native::RICH_CHILD_ID, &[MERGE_ID], false)],
    );
    assert_cursor(later.current_cursor(), &saved, 2, &[native::RICH_CHILD_ID]);
    drop(journal);
    let mut journal = case.open();
    let read = packet(case, &journal, saved.source_id(), id);
    assert_disjoint(&read);
    assert_eq!(read.head_closures(), first.admission().head_closures());
    assert_eq!(
        read.ordered_operations(),
        first.admission().ordered_operations()
    );
    select_disjoint(case);
    let before = total_state(case);
    let retry = outcome(admit_now(case, &mut journal, config, &zero), true);
    assert_disjoint(retry.admission());
    assert_eq!(retry.admission().receipt().admission_id(), id);
    assert_eq!(retry.admission().receipt().generation(), 1);
    assert_cursor(retry.current_cursor(), &saved, 2, &[native::RICH_CHILD_ID]);
    assert_eq!(
        summary(retry.admission().head_closures()),
        summary(read.head_closures())
    );
    assert_eq!(stored_packet(case, saved.source_id(), id), original);
    assert!(total_state(case) == before);
}

fn cli_json(case: &Case, config: &Config) {
    let (journal, saved, zero) = initial(case, config);
    select_disjoint(case);
    let (captured, admission, current) = capture_cli(case, config, &zero);
    let mut expected = vec![
        json!({"head_id":native::LATE_BRANCH_ID,"reached_baseline_ids":[],"reaches_root":true}),
        json!({"head_id":native::RICH_PARENT_ID,"reached_baseline_ids":[MERGE_ID],"reaches_root":false}),
    ];
    expected.sort_by(|a, b| {
        a["head_id"]
            .as_str()
            .unwrap()
            .cmp(b["head_id"].as_str().unwrap())
    });
    assert_eq!(captured["admission"]["head_closures"], json!(expected));
    assert_eq!(
        captured["admission"]["reached_baseline_ids"],
        json!([MERGE_ID])
    );
    assert_eq!(captured["admission"]["reaches_root"], true);
    assert_eq!(captured["cursor"], cursor(&current));
    let args = receipt_args(
        &case.journal_path,
        saved.source_id(),
        admission.receipt().admission_id(),
    );
    let historical = checked(case, command(case, &args, &case.test_home), None);
    assert_eq!(historical["admission"]["head_closures"], json!(expected));
    assert_eq!(historical, receipt_json(Some(&admission)));
    assert_eq!(captured["schema_version"], 1);
    assert_eq!(historical["scope"], "historical_saved_evidence");
    assert_eq!(status(case, &journal, config).cursor(), &current);
}
