use super::*;

#[derive(Serialize, Deserialize)]
pub(super) struct Remembered {
    pub(super) cursor: Json,
    pub(super) workspace: Json,
}

impl Remembered {
    pub(super) fn from_status(value: &RegisteredNativeAdmissionState) -> Self {
        Self {
            cursor: cursor(value.cursor()),
            workspace: json!({
                "name":value.registration().workspace_name(),
                "attachment_id":value.registration().attachment_id()
            }),
        }
    }
}

pub(super) fn observe_args(path: &Path, remembered: &Remembered) -> Vec<OsString> {
    let mut args = vec![
        "observe".into(),
        "--json".into(),
        "--journal".into(),
        path.as_os_str().to_owned(),
    ];
    for (flag, key) in [
        ("--expect-source", "source_id"),
        (
            "--expect-initialization-receipt",
            "initialization_receipt_id",
        ),
        ("--expect-baseline", "baseline_id"),
    ] {
        args.extend([flag.into(), remembered.cursor[key].as_str().unwrap().into()]);
    }
    args.extend([
        "--expect-generation".into(),
        remembered.cursor["generation"]
            .as_u64()
            .unwrap()
            .to_string()
            .into(),
        "--expect-workspace".into(),
        remembered.workspace["name"].as_str().unwrap().into(),
        "--expect-attachment".into(),
        remembered.workspace["attachment_id"]
            .as_str()
            .unwrap()
            .into(),
    ]);
    for head in remembered.cursor["admitted_head_ids"].as_array().unwrap() {
        args.extend(["--expect-head".into(), head.as_str().unwrap().into()]);
    }
    args
}

pub(super) fn finite_args(
    path: &Path,
    remembered: &Remembered,
    attempts: u32,
    interval: u32,
) -> Vec<OsString> {
    let mut args = observe_args(path, remembered);
    args.extend([
        "--attempts".into(),
        attempts.to_string().into(),
        "--interval-ms".into(),
        interval.to_string().into(),
    ]);
    args
}

pub(super) fn observer(case: &Case, args: &[OsString]) -> stream::Stream {
    stream::Stream::spawn(command(case, args, &case.root))
}

pub(super) fn unchanged_json(value: &RegisteredNativeAdmissionState, attempt: usize) -> Json {
    json!({
        "schema_version":1,"backend":"jj","attribution_enabled":false,
        "action":"observe","scope":"current_source","attempt":attempt,"outcome":"unchanged",
        "cursor":cursor(value.cursor()),
        "workspace":Remembered::from_status(value).workspace,
        "latest_receipt":value.latest_receipt().map(receipt)
    })
}

pub(super) fn changed_json(
    value: &DurableNativeAdmission,
    current: &RegisteredNativeAdmissionState,
    attempt: usize,
    disposition: &str,
) -> Json {
    json!({
        "schema_version":1,"backend":"jj","attribution_enabled":false,
        "action":"observe","scope":"current_source","attempt":attempt,"outcome":disposition,
        "cursor":cursor(current.cursor()),
        "workspace":Remembered::from_status(current).workspace,
        "admission":receipt_json(Some(value))["admission"]
    })
}

pub(super) fn terminal(value: Json, code: &str) {
    let message = value["error"]["message"].as_str().unwrap();
    assert!(!message.is_empty() && message.len() <= 4096);
    assert_eq!(
        value,
        json!({
            "schema_version":1,"backend":"jj","attribution_enabled":false,
            "error":{"code":code,"message":message}
        })
    );
}

pub(super) fn idle_transaction(case: &Case) {
    let conn = case.sql();
    conn.busy_timeout(Duration::ZERO).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
        .expect("observer retained a writer transaction while waiting");
}

pub(super) fn check_metadata_paths(case: &Case, value: &Json) {
    let raw = value.to_string();
    for path in [&case.repository, &case.test_home] {
        assert!(!raw.contains(path.to_str().unwrap()));
    }
}

pub(super) fn next_packet(
    case: &Case,
    journal: &JjObservationJournal,
    source: &str,
    value: &Json,
) -> DurableNativeAdmission {
    packet(
        case,
        journal,
        source,
        value["admission"]["receipt"]["admission_id"]
            .as_str()
            .unwrap(),
    )
}

pub(super) fn observe_home(path: &Path) -> std::collections::BTreeMap<std::path::PathBuf, Entry> {
    crate::jj_capture::support::manifest_excluding(
        path,
        &[
            "registration.sqlite",
            "registration.sqlite-wal",
            "registration.sqlite-shm",
            "registration-child.stdout",
            "registration-child.stderr",
        ],
    )
}
