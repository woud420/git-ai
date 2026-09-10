use super::*;

pub fn initialize_args(path: &Path) -> Vec<OsString> {
    vec![
        "initialize".into(),
        "--json".into(),
        "--journal".into(),
        path.as_os_str().to_owned(),
    ]
}

pub fn expect_args(path: &Path, expected: NativeAdmissionExpectation<'_>) -> Vec<OsString> {
    let mut args = vec![
        "capture".into(),
        "--journal".into(),
        path.as_os_str().to_owned(),
        "--expect-generation".into(),
        expected.generation.to_string().into(),
        "--expect-source".into(),
        expected.source_id.into(),
        "--expect-baseline".into(),
        expected.baseline_id.into(),
        "--expect-initialization-receipt".into(),
        expected.initialization_receipt_id.into(),
        "--json".into(),
    ];
    for head in expected.admitted_head_ids {
        args.extend(["--expect-head".into(), head.as_str().into()]);
    }
    args
}

pub fn fake_args(path: &Path, head_count: usize) -> Vec<OsString> {
    let heads: Vec<_> = (1..=head_count)
        .map(|value| format!("{value:0128x}"))
        .collect();
    expect_args(
        path,
        NativeAdmissionExpectation {
            source_id: &"0".repeat(64),
            initialization_receipt_id: &"0".repeat(64),
            baseline_id: &"0".repeat(64),
            generation: (i64::MAX - 1) as u64,
            admitted_head_ids: &heads,
        },
    )
}

pub fn invoke(case: &Case, args: &[OsString]) -> Json {
    let output = command(case, args, &case.root).output().unwrap();
    let value = crate::jj_debug_cli::success(output);
    let raw = value.to_string();
    for path in [&case.repository, &case.test_home] {
        assert!(!raw.contains(path.to_str().unwrap()));
    }
    value
}

pub fn registration_json(value: &RegisteredJjCurrentState, outcome: &str) -> Json {
    let baseline = value.baseline().receipt();
    let relation = match value.checkout_relation() {
        JjRegisteredCheckoutRelation::BaselineAnchor => "baseline_anchor",
        JjRegisteredCheckoutRelation::OutsideBaseline => "outside_baseline",
    };
    json!({
        "schema_version":1,"backend":"jj","attribution_enabled":false,
        "action":"initialize","scope":"current_source","outcome":outcome,
        "registration":{
            "source_id":value.source_id(),"initialization_receipt_id":value.initialization_receipt_id(),
            "attachment_id":value.attachment_id(),"reader_profile":baseline.reader_profile(),
            "baseline_id":baseline.baseline_id(),"baseline_generation":baseline.generation(),
            "captured_head_ids":baseline.captured_head_ids(),
            "workspace":{"name":value.workspace_name(),"checkout_relation":relation}
        }
    })
}

pub fn capture_json(
    value: &DurableNativeAdmission,
    current: &NativeAdmissionCursor,
    outcome: &str,
) -> Json {
    json!({
        "schema_version":1,"backend":"jj","attribution_enabled":false,
        "action":"capture","scope":"current_source","outcome":outcome,
        "cursor":cursor(current),"admission":receipt_json(Some(value))["admission"]
    })
}

pub fn initialize_cli(
    case: &Case,
    config: &Config,
) -> (JjObservationJournal, RegisteredJjCurrentState) {
    assert!(!case.journal_path.exists());
    assert!(!case.namespace().exists());
    let mut before = manifest(&case.repository);
    let result = invoke(case, &initialize_args(&case.journal_path));
    let journal = JjObservationJournal::open_read_only_at_path(&case.journal_path).unwrap();
    let saved = case.reopen(&journal, config).unwrap().unwrap();
    assert_eq!(result, registration_json(&saved, "installed"));
    assert_eq!(counts(case), [1; 4]);
    for (_, rows) in admission_rows(case) {
        assert!(rows.is_empty());
    }
    assert_eq!(journal.status(saved.source_id()).unwrap().generation, 0);
    before.insert(
        case.namespace()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::Directory,
    );
    before.insert(
        case.seal()
            .strip_prefix(&case.repository)
            .unwrap()
            .to_owned(),
        Entry::File(fs::read(case.seal()).unwrap()),
    );
    assert_eq!(manifest(&case.repository), before);
    (journal, saved)
}

pub fn capture_cli(
    case: &Case,
    config: &Config,
    expected: &NativeAdmissionCursor,
) -> (Json, DurableNativeAdmission, NativeAdmissionCursor) {
    let before = state(case);
    let result = invoke(
        case,
        &expect_args(&case.journal_path, expected.expectation()),
    );
    assert_eq!(state(case), before);
    let journal = JjObservationJournal::open_read_only_at_path(&case.journal_path).unwrap();
    let observed = status(case, &journal, config);
    let id = result["admission"]["receipt"]["admission_id"]
        .as_str()
        .unwrap();
    let admission = packet(case, &journal, expected.source_id(), id);
    assert_eq!(
        result,
        capture_json(&admission, observed.cursor(), "admitted")
    );
    (result, admission, observed.cursor().clone())
}
