use super::*;

pub(super) fn policy(case: &Case, config: &Config) {
    let (journal, _, _) = initial(case, config);
    let current = status(case, &journal, config);
    let remembered = Remembered::from_status(&current);
    let path = case.test_home.join(".git-ai/config.json");
    let mut file: git_ai::config::FileConfig =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    file.allowed_repositories = Some(vec![case.root.to_str().unwrap().to_owned()]);
    fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
    let mut cmd = command(
        case,
        &finite_args(&case.journal_path, &remembered, 3, 2_000),
        &case.root,
    );
    cmd.env_remove("GIT_AI_TEST_CONFIG_PATCH");
    let before = total_state(case);
    let mut process = stream::Stream::spawn(cmd);
    assert_eq!(process.next(), unchanged_json(&current, 1));
    assert!(
        total_state(case) == before,
        "file-policy positive control mutated state"
    );
    process.running();
    file.allowed_repositories = Some(vec![]);
    fs::write(&path, serde_json::to_vec(&file).unwrap()).unwrap();
    let before = total_state(case);
    terminal(process.next(), "collection_disabled");
    assert_eq!(process.finish(false).code(), Some(1));
    assert!(
        total_state(case) == before,
        "revoked policy permitted later work"
    );
}

pub(super) fn late_conflict(case: &Case, config: &Config) {
    let (mut journal, _, zero) = initial(case, config);
    let current = status(case, &journal, config);
    let remembered = Remembered::from_status(&current);
    let mut process = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 3, 2_000),
    );
    assert_eq!(process.next(), unchanged_json(&current, 1));
    process.running();
    if case.name.ends_with(":cursor") {
        let advanced = outcome(admit_now(case, &mut journal, config, &zero), false);
        assert_eq!(advanced.current_cursor().generation(), 1);
        assert_eq!(status(case, &journal, config).cursor().generation(), 1);
    } else {
        fs::remove_file(case.seal()).unwrap();
    }
    let before = total_state(case);
    terminal(process.next(), "admission_unavailable");
    assert_eq!(process.finish(false).code(), Some(1));
    assert!(
        total_state(case) == before,
        "observer refreshed a conflicting cursor or source"
    );
}

pub(super) fn history_limit(case: &Case, config: &Config) {
    let (journal, _, _) = initial(case, config);
    let current = status(case, &journal, config);
    let remembered = Remembered::from_status(&current);
    let records = if case.name.ends_with(":overflow") {
        let records = native::chain(257);
        assert_eq!(records.len(), 257);
        records
    } else {
        let record = native::rich_child();
        assert!(!op_path(case, &native::rich_parent().operation_id).exists());
        vec![record]
    };
    write_records(case, &records);
    let args = finite_args(&case.journal_path, &remembered, 3, 2_000);
    let mut process = observer(case, &args);
    assert_eq!(process.next(), unchanged_json(&current, 1));
    process.running();
    let head = records.last().unwrap().operation_id.as_str();
    select(case, &[], &[head], None);
    assert_eq!(
        capture_current_state(&case.context(), deadline())
            .unwrap()
            .checkout()
            .operation_id,
        MERGE_ID
    );
    let before = total_state(case);
    terminal(process.next(), "admission_unavailable");
    assert_eq!(process.finish(false).code(), Some(1));
    assert!(total_state(case) == before);
    let mut restarted = observer(case, &args);
    terminal(restarted.next(), "admission_unavailable");
    assert_eq!(restarted.finish(false).code(), Some(1));
    assert!(
        total_state(case) == before,
        "new observer invocation adopted a partial cutoff"
    );
}

pub(super) fn initial_refusal(case: &Case, config: &Config) {
    let remembered = if case.name.ends_with(":malformed") {
        let (journal, _, _) = initial(case, config);
        let remembered = Remembered::from_status(&status(case, &journal, config));
        fs::write(
            case.root.join(".jj/working_copy/checkout"),
            b"invalid checkout",
        )
        .unwrap();
        assert!(capture_current_state(&case.context(), deadline()).is_err());
        remembered
    } else {
        let _ = case.open();
        capture_current_state(&case.context(), deadline()).unwrap();
        assert!(!case.namespace().exists());
        Remembered {
            cursor: json!({
                "source_id":"00".repeat(32),"initialization_receipt_id":"11".repeat(32),
                "baseline_id":"22".repeat(32),"generation":0,"admitted_head_ids":[MERGE_ID]
            }),
            workspace: json!({"name":"default","attachment_id":"33".repeat(32)}),
        }
    };
    let before = total_state(case);
    let mut process = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 32, 60_000),
    );
    terminal(process.next(), "admission_unavailable");
    assert_eq!(process.finish(false).code(), Some(1));
    assert!(
        total_state(case) == before,
        "refusal initialized or modified a source"
    );
}

pub(super) fn closed_stdout(case: &Case, config: &Config) {
    let (journal, saved, _) = initial(case, config);
    let current = status(case, &journal, config);
    let remembered = Remembered::from_status(&current);
    let mut process = observer(
        case,
        &finite_args(&case.journal_path, &remembered, 3, 1_000),
    );
    assert_eq!(process.next(), unchanged_json(&current, 1));
    process.running();
    process.close_stdout();
    let a = native::rich_parent();
    select(case, std::slice::from_ref(&a), &[&a.operation_id], None);
    let before = state(case);
    process.finish(false);
    assert_eq!(state(case), before);
    let current = status(case, &journal, config);
    assert!(
        current.cursor().generation() <= 1,
        "stdout failure was followed by another admission"
    );
    if let Some(receipt) = current.latest_receipt() {
        let packet = packet(case, &journal, saved.source_id(), receipt.admission_id());
        assert_eq!(packet.ordered_operations(), std::slice::from_ref(&a));
        assert_eq!(receipt.expected_generation(), 0);
    }
}
