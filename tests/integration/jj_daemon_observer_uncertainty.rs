use super::support::Harness;
use super::*;
use git_ai::model::repository::sqlite::open_with_flags_and_memory_limits;
use git_ai::operations::daemon::DaemonConfig;
use rusqlite::{OpenFlags, TransactionBehavior, params};
use std::path::Path;

fn intent(path: &Path) -> (u64, Json) {
    let connection =
        open_with_flags_and_memory_limits(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let (revision, payload): (u64, Vec<u8>) = connection
        .query_row(
            "SELECT revision,payload FROM jj_observer_intent WHERE slot=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(payload.len() <= 512 * 1024);
    let payload: Json = serde_json::from_slice(&payload).unwrap();
    assert_eq!(payload["revision"], revision);
    (revision, payload)
}

fn unknown(value: &Json) {
    for key in ["revision", "desired_intent", "target", "session_cursor"] {
        assert!(value.get(key).is_some_and(Json::is_null), "{key}: {value}");
    }
    assert_eq!(value["last_error"]["code"], "intent_persistence");
    assert_eq!(value["last_error"]["persisted"], false);
}

#[test]
fn jj_daemon_observer_publication_failure_forgets_cached_intent_until_restart() {
    let mut h = Harness::new(true);
    h.start();
    h.ready_control("enable");
    let active = h.active(0, &h.baseline_heads());
    let saved = h.saved_rows();
    let admissions = h.admission_rows();
    let path = DaemonConfig::from_home(h.fixture.repo.test_home_path())
        .internal_dir
        .join("jj-observer-intent.sqlite");
    let (revision, mut external) = intent(&path);
    assert_eq!(active["revision"], revision);
    assert_eq!(external["enabled"], true);
    assert!(external["blocked"].is_null());
    assert_eq!(external["target"]["metadata"], h.target());
    external["revision"] = json!(revision + 1);
    external["enabled"] = json!(false);
    {
        let mut connection =
            open_with_flags_and_memory_limits(&path, OpenFlags::SQLITE_OPEN_READ_WRITE).unwrap();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        assert_eq!(
            tx.execute(
                "UPDATE jj_observer_intent SET revision=?1,payload=?2 WHERE slot=1 AND revision=?3",
                params![
                    revision + 1,
                    serde_json::to_vec(&external).unwrap(),
                    revision
                ],
            )
            .unwrap(),
            1,
        );
        tx.commit().unwrap();
    }
    assert_eq!(intent(&path), (revision + 1, external.clone()));

    let refused = h.control("disable", false);
    assert_eq!(refused["error"]["code"], "intent_persistence");
    unknown(&refused);
    assert!(refused["runtime"] == "stopping" || refused["runtime"] == "blocked");
    let stopped = h.wait(|v| v["runtime"] == "blocked" && v["in_flight"] == false);
    unknown(&stopped);
    for action in ["enable", "resume"] {
        let refused = h.control(action, false);
        assert_eq!(refused["error"]["code"], "intent_unavailable");
        unknown(&refused);
        assert_eq!(refused["runtime"], "blocked");
        assert_eq!(refused["in_flight"], false);
    }
    assert_eq!(intent(&path), (revision + 1, external.clone()));
    assert_eq!(h.saved_rows(), saved);
    assert_eq!(h.admission_rows(), admissions);

    h.fixture.repo.restart_dedicated_daemon_for_test();
    let reloaded = h.wait(|v| v["runtime"] == "disabled" && v["in_flight"] == false);
    assert_eq!(reloaded["revision"], revision + 1);
    assert_eq!(reloaded["desired_intent"], "disabled");
    assert_eq!(reloaded["target"], h.target());
    assert!(reloaded["session_cursor"].is_null());
    assert!(reloaded["last_error"].is_null());
    assert_eq!(intent(&path), (revision + 1, external));
    h.ready_control("resume");
    let resumed = h.active(0, &h.baseline_heads());
    assert_eq!(resumed["revision"], revision + 2);
    assert_eq!(h.saved_rows(), saved);
    assert_eq!(h.admission_rows(), admissions);
    h.disabled();
}
