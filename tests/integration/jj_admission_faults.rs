use super::*;

pub fn unregistered(case: &Case, config: &Config) {
    let mut journal = case.open();
    let ids = vec![MERGE_ID.to_owned()];
    let (source, initialization, baseline) = if case.name.ends_with(":native_only") {
        let saved = installed(case.register(&mut journal, config).unwrap());
        case.sql()
            .execute_batch("DELETE FROM jj_native_workspaces; DELETE FROM jj_native_registrations;")
            .unwrap();
        (
            saved.source_id().to_owned(),
            saved.initialization_receipt_id().to_owned(),
            saved.baseline().receipt().baseline_id().to_owned(),
        )
    } else {
        ("11".repeat(32), "22".repeat(32), "33".repeat(32))
    };
    capture_current_state(&case.context(), deadline()).unwrap();
    failed(checked_status(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    ));
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        NativeAdmissionExpectation {
            source_id: &source,
            initialization_receipt_id: &initialization,
            baseline_id: &baseline,
            generation: 0,
            admitted_head_ids: &ids,
        },
        deadline(),
        &mut admission_budget(),
    ));
    failed(checked_known(
        case,
        &journal,
        &source,
        &"ab".repeat(32),
        deadline(),
        &mut admission_budget(),
    ));
    assert!(admission_rows(case).iter().all(|(_, rows)| rows.is_empty()));
    if case.name.ends_with(":unregistered") {
        assert!(!case.namespace().exists());
    }
}

pub fn expectations(case: &Case, config: &Config) {
    let (mut journal, _saved, zero) = initial(case, config);
    for kind in [
        "source",
        "initialization",
        "baseline",
        "source_format",
        "head_format",
        "empty",
        "duplicate",
        "over_heads",
        "root",
        "generation",
    ] {
        let other = "ac".repeat(32);
        let bad = "Z".repeat(64);
        let mut heads = zero.admitted_head_ids().to_vec();
        let mut input = zero.expectation();
        match kind {
            "source" => input.source_id = &other,
            "initialization" => input.initialization_receipt_id = &other,
            "baseline" => input.baseline_id = &other,
            "source_format" => input.source_id = &bad,
            "head_format" => heads = vec!["a".repeat(127)],
            "empty" => heads.clear(),
            "duplicate" => heads.push(MERGE_ID.to_owned()),
            "over_heads" => heads = (1..=33).map(|n| format!("{n:0128x}")).collect(),
            "root" => heads = vec!["0".repeat(128)],
            "generation" => input.generation = (i64::MAX as u64) + 1,
            _ => unreachable!(),
        }
        input.admitted_head_ids = &heads;
        failed(admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            input,
            deadline(),
            &mut admission_budget(),
        ));
    }
}

pub fn missing_parent(case: &Case, config: &Config) {
    let (mut journal, _saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    select(case, std::slice::from_ref(&b), &[&b.operation_id], None);
    fs::remove_file(op_path(case, &a.operation_id)).unwrap();
    capture_current_state(&case.context(), deadline()).unwrap();
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        first.current_cursor().expectation(),
        deadline(),
        &mut admission_budget(),
    ));
    case.write_evidence(&a);
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    assert_eq!(second.admission().ordered_operations(), [a, b]);
}

pub fn historical_gc(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(case, &[a.clone(), b.clone()], &[&b.operation_id], None);
    let admitted = outcome(admit_now(case, &mut journal, config, &zero), false);
    let id = admitted.admission().receipt().admission_id().to_owned();
    for id in [MERGE_ID, &a.operation_id, &b.operation_id] {
        fs::remove_file(op_path(case, id)).unwrap();
    }
    fs::remove_file(view_path(case, RICH_ID)).unwrap();
    fs::remove_file(case.seal()).unwrap();
    let read = packet(case, &journal, saved.source_id(), &id);
    assert_eq!(read.ordered_operations(), [a, b]);
    failed(checked_status(
        case,
        &journal,
        &case.context(),
        config,
        deadline(),
        &mut admission_budget(),
    ));
    failed(admit_checked(
        case,
        &mut journal,
        &case.context(),
        config,
        admitted.current_cursor().expectation(),
        deadline(),
        &mut admission_budget(),
    ));
}

pub fn expired(case: &Case, config: &Config) {
    let (mut journal, saved, zero) = initial(case, config);
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    let expired = Instant::now() - Duration::from_secs(1);
    for error in [
        failed(checked_status(
            case,
            &journal,
            &case.context(),
            config,
            expired,
            &mut admission_budget(),
        )),
        failed(checked_known(
            case,
            &journal,
            saved.source_id(),
            first.admission().receipt().admission_id(),
            expired,
            &mut admission_budget(),
        )),
        failed(admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            zero.expectation(),
            expired,
            &mut admission_budget(),
        )),
    ] {
        assert!(
            error.to_string().to_ascii_lowercase().contains("deadline"),
            "{error}"
        );
    }
}

pub fn bound(case: &Case, config: &Config) {
    let (mut journal, _saved, zero) = initial(case, config);
    if case.name.ends_with(":count") {
        let records = native::chain(256);
        let last = &records.last().unwrap().operation_id;
        select(case, &records, &[last], None);
        let first = outcome(admit_now(case, &mut journal, config, &zero), false);
        assert_eq!(first.admission().ordered_operations(), records);
        let over = native::chain(257);
        select(case, &over, &[&over.last().unwrap().operation_id], None);
        failed(admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            first.current_cursor().expectation(),
            deadline(),
            &mut admission_budget(),
        ));
    } else {
        let records = native::raw_chain(false);
        assert_eq!(native::raw_size(&records), 8 * 1024 * 1024);
        select(
            case,
            &records,
            &[&records.last().unwrap().operation_id],
            None,
        );
        let collected = collect(case, &journal, config);
        assert_eq!(collected.ordered_operations(), records);
        // Native raw8MiB acceptance leaves no room for the canonical packet framing.
        failed(admit_checked(
            case,
            &mut journal,
            &case.context(),
            config,
            zero.expectation(),
            deadline(),
            &mut admission_budget(),
        ));
    }
}
