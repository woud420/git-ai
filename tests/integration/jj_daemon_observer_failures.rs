use super::support::Harness;
use super::*;

#[test]
fn jj_daemon_observer_enable_refuses_missing_unregistered_and_denied_sources() {
    for kind in ["missing", "unregistered", "denied"] {
        let mut h = Harness::new(true);
        let saved = h.saved_rows();
        h.start();
        if kind == "missing" {
            h.journal = h
                .fixture
                .repo
                .test_home_path()
                .join("missing-observer/journal.sqlite");
        } else if kind == "unregistered" {
            h.journal = h.fixture.repo.test_home_path().join("unregistered.sqlite");
            drop(JjObservationJournal::open_at_path(&h.journal).unwrap());
        } else {
            h.fixture.repo.patch_git_ai_config(|patch| {
                patch.allowed_repositories =
                    Some(vec!["https://example.invalid/not-this-source".to_owned()]);
            });
        }
        let files = manifest(h.fixture.repo.path());
        let before = if kind == "missing" {
            None
        } else {
            Some(h.admission_rows())
        };
        let value = h.control("enable", false);
        let expected = if kind == "missing" {
            "journal_unavailable"
        } else {
            "admission_unavailable"
        };
        assert_eq!(value["error"]["code"], expected, "{kind}: {value}");
        let status = h.wait(|v| v["in_flight"] == false);
        assert_eq!(status["revision"], 0);
        assert!(status["target"].is_null());
        assert_eq!(status["runtime"], "disabled");
        assert_eq!(manifest(h.fixture.repo.path()), files);
        if let Some(before) = before {
            assert_eq!(h.admission_rows(), before);
        } else {
            assert!(!h.journal.parent().unwrap().exists());
        }
        h.journal = h.fixture.repo.test_home_path().join("registration.sqlite");
        assert_eq!(h.saved_rows(), saved);
    }
}

#[test]
fn jj_daemon_observer_policy_revocation_and_original_cutoff_exhaustion_persist_blocks() {
    let mut policy = Harness::new(true);
    policy.start();
    policy.ready_control("enable");
    policy.active(0, &policy.baseline_heads());
    let before = policy.admission_rows();
    let saved = policy.saved_rows();
    let path = policy
        .fixture
        .repo
        .test_home_path()
        .join(".git-ai/config.json");
    let mut config: git_ai::config::FileConfig =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config.allowed_repositories = Some(vec![]);
    let pending = path.with_file_name("observer-denied-config.tmp");
    fs::write(&pending, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::rename(&pending, &path).unwrap();
    let blocked = policy.blocked("collection_disabled");
    assert_eq!(policy.admission_rows(), before);
    assert_eq!(policy.saved_rows(), saved);
    // Keep restart's TestRepo-owned config projection consistent after the job stops.
    policy
        .fixture
        .repo
        .patch_git_ai_config(|patch| patch.allowed_repositories = Some(vec![]));
    policy.fixture.repo.restart_dedicated_daemon_for_test();
    assert_eq!(
        policy.blocked("collection_disabled")["revision"],
        blocked["revision"]
    );
    let resume = policy.control("resume", false);
    assert_eq!(resume["error"]["code"], "collection_disabled");
    assert_eq!(resume["revision"], blocked["revision"]);
    assert_eq!(policy.admission_rows(), before);
    policy.disabled();

    let mut limited = Harness::new(false);
    let records = native::chain(257);
    for record in &records {
        limited.fixture.write_evidence(record);
    }
    limited
        .fixture
        .set_heads(&[&records.last().unwrap().operation_id]);
    // Keep MERGE checkout's valid own view; the minimal chain view has no workspace.
    capture_current_state(&limited.fixture.context(), deadline()).unwrap();
    limited.start();
    let saved = limited.saved_rows();
    limited.ready_control("enable");
    let first = limited.blocked("admission_unavailable");
    assert_eq!(limited.packet_count(), 0);
    limited.fixture.repo.restart_dedicated_daemon_for_test();
    assert_eq!(
        limited.blocked("admission_unavailable")["revision"],
        first["revision"]
    );
    limited.ready_control("resume");
    limited.blocked("admission_unavailable");
    assert_eq!(limited.packet_count(), 0);
    assert_eq!(limited.saved_rows(), saved);
    limited.disabled();
}
