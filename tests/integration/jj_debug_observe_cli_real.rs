use super::*;
use crate::debug_context::{jj, read_jj_fixture, require_pinned_jj};

#[derive(Serialize, Deserialize)]
struct Saved {
    remembered: Remembered,
    anchors: Vec<JjOperationEvidence>,
}

#[derive(Serialize, Deserialize)]
struct Observed {
    records: Vec<Json>,
    expected: JjOperationEvidence,
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_debug_observe_cli_real_colocated_streams_a_native_change_during_wait() {
    fixture(true);
}

#[test]
#[ignore = "requires explicit GIT_AI_TEST_JJ_BINARY qualification lane"]
fn jj_debug_observe_cli_real_noncolocated_streams_a_native_change_during_wait() {
    fixture(false);
}

fn fixture(colocated: bool) {
    require_pinned_jj();
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    let root = if colocated {
        jj(&repo, repo.path(), &["git", "init", "--colocate"]);
        repo.path().to_owned()
    } else {
        let root = repo.path().join("real observer workspace");
        jj(
            &repo,
            repo.path(),
            &["git", "init", "--no-colocate", root.to_str().unwrap()],
        );
        root
    };
    jj(&repo, &root, &["describe", "-m", "observer baseline"]);
    let mut fixture = Fixture {
        repo,
        repo_dir: root.join(".jj/repo"),
        root,
    };
    fixture_case("observe:real_install", &mut fixture, Policy::Root);
    let home = fixture.repo.test_home_path();
    let saved: Saved = serde_json::from_slice(&read_jj_fixture(
        &home.join("observe-real-saved.json"),
        128 * 1024,
    ))
    .unwrap();
    let seal = read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024);
    let subdir = fixture.root.join("observer subdir");
    fs::create_dir(&subdir).unwrap();
    let mut cmd = fixture
        .repo
        .git_ai_command_without_pre_sync_for_test(&["debug", "jj"], &[]);
    cmd.args(finite_args(
        &home.join("registration.sqlite"),
        &saved.remembered,
        3,
        5_000,
    ))
    .current_dir(&subdir)
    .env("GIT_AI_DEBUG", "0")
    .env("GIT_CONFIG_COUNT", "0")
    .env_remove("GIT_CONFIG_PARAMETERS")
    .env_remove("GIT_CONFIG")
    .env_remove("GIT_DIR")
    .env_remove("GIT_COMMON_DIR")
    .env_remove("GIT_WORK_TREE")
    .env_remove("GIT_TRACE2_EVENT")
    .env_remove("GIT_AI");
    let home_state = || observe_home(home);
    let home_before = home_state();
    let before = manifest(fixture.repo.path());
    let mut process = stream::Stream::spawn(cmd);
    let first = process.next();
    assert_eq!(
        first,
        json!({
            "schema_version":1,"backend":"jj","attribution_enabled":false,
            "action":"observe","scope":"current_source","attempt":1,"outcome":"unchanged",
            "cursor":saved.remembered.cursor,"workspace":saved.remembered.workspace,"latest_receipt":null
        })
    );
    let paused = process.pause();
    assert_eq!(manifest(fixture.repo.path()), before);
    assert_eq!(home_state(), home_before);
    jj(
        &fixture.repo,
        &fixture.root,
        &["describe", "-m", "observer change while waiting"],
    );
    let captured = capture_current_state(&fixture.context(), deadline()).unwrap();
    assert_eq!(captured.anchors().len(), 1);
    let expected = captured.anchors()[0].clone();
    assert_eq!(
        json!(expected.parent_ids),
        saved.remembered.cursor["admitted_head_ids"]
    );
    assert_ne!(
        json!([&expected.operation_id]),
        saved.remembered.cursor["admitted_head_ids"]
    );
    fs::write(
        fixture.root.join("dirty-observe.txt"),
        b"never snapshotted by observe\n",
    )
    .unwrap();
    let before = manifest(fixture.repo.path());
    let home_before = home_state();
    drop(paused);
    let second = process.next();
    let third = process.next();
    process.finish(true);
    assert_eq!(manifest(fixture.repo.path()), before);
    assert_eq!(home_state(), home_before);
    assert_eq!(
        read_jj_fixture(&fixture.repo_dir.join("git-ai/registration"), 1024),
        seal
    );
    let observed = Observed {
        records: vec![first, second, third],
        expected,
    };
    fs::write(
        home.join("observe-real-observed.json"),
        serde_json::to_vec(&observed).unwrap(),
    )
    .unwrap();
    fixture_case("observe:real_verify", &mut fixture, Policy::Root);
}

pub(super) fn install_real(case: &Case, config: &Config) {
    let (journal, registered) =
        crate::jj_capture::registration::history::support::install(case, config);
    let current = status(case, &journal, config);
    assert_eq!(current.cursor().generation(), 0);
    assert!(current.latest_receipt().is_none());
    let saved = Saved {
        remembered: Remembered::from_status(&current),
        anchors: registered.baseline().anchors().to_vec(),
    };
    fs::write(
        case.test_home.join("observe-real-saved.json"),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
}

pub(super) fn verify_real(case: &Case, config: &Config) {
    let saved: Saved = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("observe-real-saved.json"),
        128 * 1024,
    ))
    .unwrap();
    let observed: Observed = serde_json::from_slice(&read_jj_fixture(
        &case.test_home.join("observe-real-observed.json"),
        128 * 1024,
    ))
    .unwrap();
    let before = total_state(case);
    let journal = JjObservationJournal::open_read_only_at_path(&case.journal_path).unwrap();
    let current = status(case, &journal, config);
    assert_eq!(current.cursor().generation(), 1);
    assert_eq!(current.registration().baseline().anchors(), saved.anchors);
    assert_eq!(
        current.registration().baseline().receipt().baseline_id(),
        saved.remembered.cursor["baseline_id"].as_str().unwrap()
    );
    assert_eq!(
        Remembered::from_status(&current).workspace,
        saved.remembered.workspace
    );
    assert_eq!(
        current.cursor().admitted_head_ids(),
        std::slice::from_ref(&observed.expected.operation_id)
    );
    let packet = next_packet(
        case,
        &journal,
        current.registration().source_id(),
        &observed.records[1],
    );
    assert_eq!(
        packet.ordered_operations(),
        std::slice::from_ref(&observed.expected)
    );
    assert_eq!(observed.records.len(), 3);
    assert_eq!(
        observed.records[1],
        changed_json(&packet, &current, 2, "admitted")
    );
    assert_eq!(observed.records[2], unchanged_json(&current, 3));
    assert_eq!(
        fs::read(case.root.join("dirty-observe.txt")).unwrap(),
        b"never snapshotted by observe\n"
    );
    assert!(total_state(case) == before);
}
