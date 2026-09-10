use super::*;
use std::collections::BTreeMap;

pub(super) const ALL_PHASES: [AdmissionPhase; 3] = [
    AdmissionPhase::HistoryCollected,
    AdmissionPhase::PriorSnapshotVerified,
    AdmissionPhase::ReadbackVerified,
];

pub(super) struct Case {
    pub fixture: Fixture,
    pub db: PathBuf,
    pub config: Config,
    pub registered: RegisteredJjCurrentState,
}

impl Case {
    pub fn new() -> (Self, JjObservationJournal, NativeAdmissionCursor) {
        let fixture = Fixture::new();
        fs::write(
            fixture.root.join(".git/config"),
            b"[core]\nrepositoryformatversion = 0\nbare = false\n",
        )
        .unwrap();
        let config = create_test_config(vec![fixture.root.to_str().unwrap().to_owned()], vec![]);
        let policy = crate::operations::git::repository::load_repository_policy_context_for_paths(
            &fixture.root,
            &fixture.root.join(".git"),
            &fixture.root.join(".git"),
        )
        .unwrap();
        assert!(policy.is_collection_allowed(&config));
        // Keep the journal outside the ancestor that a retained-path fault replaces.
        let db = fixture.ancestor.parent().unwrap().join("admission.sqlite");
        let mut journal = JjObservationJournal::open_at_path(&db).unwrap();
        let registered = match register_current_state(
            &mut journal,
            &fixture.context(),
            &config,
            deadline(),
            &mut reads(),
        )
        .unwrap()
        {
            JjRegistrationOutcome::Installed(value) => value,
            JjRegistrationOutcome::AlreadyRegistered(_) => panic!("fixture was already registered"),
        };
        assert_eq!(registered.baseline().anchors(), &[fixtures::merge()]);
        assert_eq!(registered.workspace_name(), "default");
        assert!(fixture.repo.join("git-ai/registration").is_file());
        let status = read_registered_admission_state(
            &journal,
            &fixture.context(),
            &config,
            deadline(),
            &mut reads(),
        )
        .unwrap();
        assert_eq!(status.cursor().generation(), 0);
        assert!(status.latest_receipt().is_none());
        assert_eq!(admission_counts(&db), [0, 0]);
        let cursor = status.cursor().clone();
        (
            Self {
                fixture,
                db,
                config,
                registered,
            },
            journal,
            cursor,
        )
    }

    pub fn select(&self, records: &[JjOperationEvidence], head: &JjOperationEvidence) {
        write_records(&self.fixture, records);
        self.fixture.set_heads(&[&head.operation_id]);
        crate::operations::jj::capture::capture_current_state(&self.fixture.context(), deadline())
            .unwrap();
    }
}

pub(super) fn reads() -> ReadBudget {
    ReadBudget::new(48 * 1024 * 1024)
}

pub(super) struct Hook<F> {
    pub phases: Vec<AdmissionPhase>,
    callback: F,
    pub expires: Option<AdmissionPhase>,
    now: Instant,
}
impl<F: FnMut(AdmissionPhase)> Hook<F> {
    pub fn new(callback: F) -> Self {
        Self {
            phases: Vec::new(),
            callback,
            expires: None,
            now: Instant::now(),
        }
    }
}
impl<F: FnMut(AdmissionPhase)> AdmissionHooks for Hook<F> {
    fn phase(&mut self, phase: AdmissionPhase) {
        self.phases.push(phase);
        (self.callback)(phase);
        if self.expires == Some(phase) {
            self.now += Duration::from_secs(120);
        }
    }
    fn now(&mut self) -> Instant {
        self.now
    }
}

pub(super) fn run(
    case: &Case,
    journal: &mut JjObservationJournal,
    expected: &NativeAdmissionCursor,
    hook: &mut impl AdmissionHooks,
) -> Result<NativeAdmissionOutcome, JjNativeAdmissionError> {
    admit_with_hooks(
        journal,
        &case.fixture.context(),
        &case.config,
        expected.expectation(),
        deadline(),
        &mut reads(),
        hook,
    )
}

pub(super) fn admitted(outcome: NativeAdmissionOutcome) -> RegisteredNativeAdmission {
    match outcome {
        NativeAdmissionOutcome::Admitted(value) => value,
        NativeAdmissionOutcome::AlreadyAdmitted(_) => panic!("unexpected historical receipt"),
    }
}

pub(super) fn refuse<T>(result: Result<T, JjNativeAdmissionError>) -> JjNativeAdmissionError {
    match result {
        Ok(_) => panic!("coordinator accepted a late operational-source fault"),
        Err(error) => error,
    }
}

pub(super) fn assert_packet(
    result: &RegisteredNativeAdmission,
    case: &Case,
    records: &[JjOperationEvidence],
    head: &str,
) {
    assert_eq!(result.current_cursor().generation(), 1);
    assert_eq!(result.current_cursor().admitted_head_ids(), &[head]);
    assert_eq!(
        result.registration().initialization_receipt_id(),
        case.registered.initialization_receipt_id()
    );
    assert_eq!(result.admission().ordered_operations(), records);
    assert_eq!(result.admission().receipt().captured_head_ids(), &[head]);
    assert_eq!(
        result.admission().reached_baseline_ids(),
        case.registered.baseline().receipt().captured_head_ids()
    );
    assert!(!result.admission().reaches_root());
    for record in result.admission().ordered_operations() {
        crate::operations::jj::evidence::verify_evidence(JJ_OBSERVATION_READER_PROFILE, record)
            .unwrap();
    }
}

type SqlRows = Vec<(String, Vec<Vec<SqlValue>>)>;

pub(super) fn sql_snapshot(path: &Path, include_admissions: bool) -> SqlRows {
    let conn = open_with_memory_limits(path).unwrap();
    let mut tables = vec![
        "schema_metadata",
        "jj_sources",
        "jj_operations",
        "jj_views",
        "jj_batches",
        "jj_native_baselines",
        "jj_native_sources",
        "jj_native_registrations",
        "jj_native_workspaces",
    ];
    if include_admissions {
        tables.extend(["jj_native_admissions", "jj_native_admission_states"]);
    }
    tables
        .into_iter()
        .map(|table| {
            let mut statement = conn
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2 LIMIT 17"))
                .unwrap();
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns)
                        .map(|i| row.get(i))
                        .collect::<rusqlite::Result<Vec<SqlValue>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert!(rows.len() < 17);
            (table.to_owned(), rows)
        })
        .collect()
}

pub(super) fn admission_counts(path: &Path) -> [usize; 2] {
    let conn = open_with_memory_limits(path).unwrap();
    ["jj_native_admissions", "jj_native_admission_states"].map(|table| {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    })
}

pub(super) fn filesystem(fixture: &Fixture) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let root = fixture.ancestor.parent().unwrap();
    let mut found = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    let mut total = 0usize;
    while let Some(directory) = queue.pop() {
        for entry in fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if relative.to_str().unwrap().starts_with("admission.sqlite") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).unwrap();
            assert!(!metadata.file_type().is_symlink());
            assert!(found.len() < 256);
            if metadata.is_dir() {
                queue.push(path);
                found.insert(relative, None);
            } else {
                assert!(metadata.is_file());
                total += usize::try_from(metadata.len()).unwrap();
                assert!(total <= 16 * 1024 * 1024);
                found.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    found
}
