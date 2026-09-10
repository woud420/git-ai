use super::support::Harness;
use super::*;

#[test]
fn jj_daemon_observer_restart_missing_journal_or_source_blocks_without_adoption() {
    for missing in ["journal", "source"] {
        let mut h = Harness::new(true);
        h.start();
        h.ready_control("enable");
        let active = h.active(0, &h.baseline_heads());
        let saved = h.saved_rows();
        let admissions = h.admission_rows();
        h.fixture.repo.shutdown_dedicated_daemon_for_test();

        // No test SQL handle survives these moves, and the TestRepo owns the
        // stopped daemon. Move sidecars as names only if SQLite retained any.
        let mut moved = Vec::new();
        if missing == "journal" {
            for suffix in ["", "-wal", "-shm"] {
                let from = h
                    .journal
                    .with_file_name(format!("registration.sqlite{suffix}"));
                if from.exists() {
                    let to = from.with_file_name(format!("saved-registration.sqlite{suffix}"));
                    fs::rename(&from, &to).unwrap();
                    moved.push((from, to));
                }
            }
            assert!(!h.journal.exists());
        } else {
            let from = h.fixture.repo_dir.clone();
            let to = from.with_file_name("saved-repo");
            fs::rename(&from, &to).unwrap();
            moved.push((from, to));
        }
        h.fixture.repo.restart_dedicated_daemon_for_test();
        let blocked = h.wait(|v| {
            v["runtime"] == "blocked"
                && v["in_flight"] == false
                && v["last_error"]["persisted"] == true
        });
        assert_eq!(blocked["target"], h.target());
        assert_eq!(blocked["desired_intent"], "enabled");
        assert!(blocked["revision"].as_u64().unwrap() > active["revision"].as_u64().unwrap());
        assert!(blocked["session_cursor"].is_null());
        let code = blocked["last_error"]["code"].as_str().unwrap();
        let message = blocked["last_error"]["message"].as_str().unwrap();
        assert!(!code.is_empty() && code.len() <= 64);
        assert!(!message.is_empty() && message.len() <= 4096);
        if missing == "journal" {
            assert_eq!(code, "journal_unavailable");
        }
        for (from, to) in &moved {
            assert!(!from.exists(), "observer recreated the missing {missing}");
            fs::rename(to, from).unwrap();
        }
        assert_eq!(h.saved_rows(), saved);
        assert_eq!(h.admission_rows(), admissions);
        let still_blocked = h.control("status", true);
        assert_eq!(still_blocked["runtime"], "blocked");
        assert_eq!(still_blocked["revision"], blocked["revision"]);
        assert_eq!(still_blocked["last_error"], blocked["last_error"]);
        h.ready_control("resume");
        h.active(0, &h.baseline_heads());
        assert_eq!(h.packet_count(), 0);
        assert_eq!(h.saved_rows(), saved);
        h.disabled();
    }
}
