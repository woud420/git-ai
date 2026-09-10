use super::*;

pub fn command(case: &Case, args: &[OsString], cwd: &Path) -> Command {
    let raw = fs::read(case.test_home.join("jj-cli-binary.json")).unwrap();
    assert!(raw.len() < 16 * 1024);
    let binary: std::path::PathBuf = serde_json::from_slice(&raw).unwrap();
    assert!(binary.is_file());
    let mut command = Command::new(binary);
    command
        .args(["debug", "jj"])
        .args(args)
        .current_dir(cwd)
        .env("GIT_AI_DEBUG", "0")
        .env_remove("GIT_AI")
        .env_remove("GIT_TRACE2_EVENT")
        .stdin(std::process::Stdio::null());
    command
}

pub fn status_args(path: &Path) -> Vec<OsString> {
    vec![
        "status".into(),
        "--json".into(),
        "--journal".into(),
        path.as_os_str().to_owned(),
    ]
}

pub fn receipt_args(path: &Path, source: &str, id: &str) -> Vec<OsString> {
    vec![
        "receipt".into(),
        "--admission".into(),
        id.into(),
        "--journal".into(),
        path.as_os_str().to_owned(),
        "--json".into(),
        "--source".into(),
        source.into(),
    ]
}

pub fn checked(case: &Case, mut command: Command, code: Option<&str>) -> Json {
    let before = total_state(case);
    let output = command.output().unwrap();
    assert!(
        total_state(case) == before,
        "CLI changed logical storage or fixture files"
    );
    let value = match code {
        Some(code) => crate::jj_debug_cli::error(output, code),
        None => crate::jj_debug_cli::success(output),
    };
    let raw = value.to_string();
    for private in [&case.repository, &case.test_home] {
        assert!(
            !raw.contains(private.to_str().unwrap()),
            "JSON exposed a fixture path"
        );
    }
    value
}

pub fn cursor(value: &NativeAdmissionCursor) -> Json {
    json!({
        "source_id":value.source_id(),
        "initialization_receipt_id":value.initialization_receipt_id(),
        "reader_profile":value.reader_profile(),"baseline_id":value.baseline_id(),
        "baseline_generation":value.baseline_generation(),"generation":value.generation(),
        "admitted_head_ids":value.admitted_head_ids()
    })
}

pub fn receipt(value: &NativeAdmissionReceipt) -> Json {
    json!({
        "admission_id":value.admission_id(),"source_id":value.source_id(),
        "initialization_receipt_id":value.initialization_receipt_id(),
        "reader_profile":value.reader_profile(),"baseline_id":value.baseline_id(),
        "baseline_generation":value.baseline_generation(),"generation":value.generation(),
        "expected_generation":value.expected_generation(),
        "expected_admitted_head_ids":value.expected_admitted_head_ids(),
        "captured_head_ids":value.captured_head_ids()
    })
}

pub fn status_json(value: &RegisteredNativeAdmissionState) -> Json {
    let relation = match value.registration().checkout_relation() {
        JjRegisteredCheckoutRelation::BaselineAnchor => "baseline_anchor",
        JjRegisteredCheckoutRelation::OutsideBaseline => "outside_baseline",
    };
    json!({
        "schema_version":1,"backend":"jj","attribution_enabled":false,
        "action":"status","scope":"current_source","cursor":cursor(value.cursor()),
        "latest_receipt":value.latest_receipt().map(receipt),
        "workspace":{"name":value.registration().workspace_name(),"checkout_relation":relation}
    })
}

pub fn receipt_json(value: Option<&DurableNativeAdmission>) -> Json {
    json!({
        "schema_version":1,"backend":"jj","attribution_enabled":false,
        "action":"receipt","scope":"historical_saved_evidence",
        "admission":value.map(|value| json!({
            "receipt":receipt(value.receipt()),"operation_count":value.ordered_operations().len(),
            "reached_baseline_ids":value.reached_baseline_ids(),"reaches_root":value.reaches_root(),
            "head_closures":value.head_closures().iter().map(|head| json!({
                "head_id":head.head_id(),"reached_baseline_ids":head.reached_baseline_ids(),
                "reaches_root":head.reaches_root()
            })).collect::<Vec<_>>()
        }))
    })
}

pub fn two(
    case: &Case,
    config: &Config,
) -> (
    JjObservationJournal,
    RegisteredJjCurrentState,
    RegisteredNativeAdmission,
    RegisteredNativeAdmission,
) {
    let (mut journal, saved, zero) = initial(case, config);
    let a = native::rich_parent();
    let b = native::rich_child();
    select(
        case,
        &[a.clone(), b.clone()],
        &[&a.operation_id],
        Some(&a.operation_id),
    );
    let first = outcome(admit_now(case, &mut journal, config, &zero), false);
    case.heads(&[&b.operation_id]);
    case.write_checkout(&b.operation_id);
    let second = outcome(
        admit_now(case, &mut journal, config, first.current_cursor()),
        false,
    );
    (journal, saved, first, second)
}

pub fn no_creation(case: &Case, mut command: Command, code: &str, missing: &Path) {
    assert!(!missing.exists());
    let before = manifest(&case.repository);
    let config = fs::read(case.test_home.join(".git-ai/config.json")).unwrap();
    let value = crate::jj_debug_cli::error(command.output().unwrap(), code);
    assert!(!value.to_string().contains(case.test_home.to_str().unwrap()));
    assert!(!missing.exists());
    assert!(!case.namespace().exists());
    assert_eq!(manifest(&case.repository), before);
    assert_eq!(
        fs::read(case.test_home.join(".git-ai/config.json")).unwrap(),
        config
    );
}
