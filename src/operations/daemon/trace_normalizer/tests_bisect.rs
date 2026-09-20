use super::bisect::BisectCheckoutCapture;
use serde_json::{Value, json};
use std::path::Path;

#[tokio::test]
async fn bisect_ingress_keeps_mutation_fences_without_capturing_reflog_offsets() {
    use crate::operations::daemon::{
        ActorDaemonCoordinator, TRACE_ROOT_REFLOG_START_OFFSETS_FIELD,
    };
    let coordinator = ActorDaemonCoordinator::new();
    let directory = tempfile::tempdir().unwrap();
    let git_dir = directory.path().join(".git");
    crate::operations::git::test_utils::seed_valid_git_dir(&git_dir);
    std::fs::create_dir_all(git_dir.join("logs")).unwrap();
    std::fs::write(git_dir.join("logs/HEAD"), "existing log\n").unwrap();
    let sid = "bisect-ingress";
    for mut payload in [
        json!({"event":"start", "sid":sid, "argv":["git", "bisect", "good"], "worktree":directory.path()}),
        json!({"event":"def_repo", "sid":sid, "repo":1, "worktree":directory.path()}),
    ] {
        assert!(coordinator.prepare_trace_payload_for_ingest(&mut payload));
        assert!(payload.get(TRACE_ROOT_REFLOG_START_OFFSETS_FIELD).is_none());
    }
    assert_eq!(
        coordinator
            .trace_ingress_state
            .lock()
            .unwrap()
            .root_mutating
            .get(sid),
        Some(&true)
    );
}

fn frames() -> Vec<Value> {
    vec![
        json!({"event":"start", "sid":"root/child", "argv":["git", "checkout", "-q", "target", "--"]}),
        json!({"event":"def_repo", "sid":"root/child", "repo":1, "worktree":"/repo"}),
        json!({"event":"cmd_name", "sid":"root/child", "name":"checkout"}),
        json!({"event":"exit", "sid":"root/child", "code":0}),
    ]
}

fn capture(args: &[&str], events: Vec<Value>) -> BisectCheckoutCapture {
    let argv = args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
    let mut capture = BisectCheckoutCapture::new(&argv, Some(Path::new("/repo")));
    for (i, event) in events.iter().enumerate() {
        capture.observe(
            event,
            event["sid"].as_str().unwrap(),
            "root",
            100 + i as u128,
        );
    }
    capture
}

#[test]
fn bisect_capture_requires_a_successful_child_in_the_parent_worktree() {
    let capture = capture(&["git", "bisect", "good"], frames());
    let receipt = capture.receipt(0).unwrap();
    assert_eq!(receipt.target, "target");
    assert_eq!((receipt.started_at_ns, receipt.finished_at_ns), (100, 103));
    assert!(capture.receipt(1).is_none());
}

#[test]
fn bisect_capture_does_not_promote_partial_or_ambiguous_children() {
    for boundary in [
        "missing_start",
        "missing_repo",
        "missing_name",
        "missing_exit",
        "wrong_name",
        "failed",
        "foreign",
        "two",
        "nested",
        "path_checkout",
    ] {
        let mut events = frames();
        match boundary {
            "missing_start" => {
                events.remove(0);
            }
            "missing_repo" => {
                events.remove(1);
            }
            "missing_name" => {
                events.remove(2);
            }
            "missing_exit" => {
                events.remove(3);
            }
            "wrong_name" => events[2]["name"] = json!("reset"),
            "failed" => events[3]["code"] = json!(1),
            "foreign" => events[1]["worktree"] = json!("/other"),
            "two" => events.extend(frames()),
            "nested" => {
                let mut nested = frames()[0].clone();
                nested["sid"] = json!("root/child/nested");
                events.push(nested);
            }
            "path_checkout" => {
                events[0]["argv"] = json!(["git", "checkout", "-q", "target", "--", "file"])
            }
            _ => unreachable!(),
        }
        assert!(
            capture(&["git", "bisect", "good"], events)
                .receipt(0)
                .is_none(),
            "{boundary}"
        );
    }
}

#[test]
fn bisect_capture_leaves_run_replay_custom_terms_and_other_commands_unsupported() {
    for args in [
        vec!["git", "bisect", "run", "true"],
        vec!["git", "bisect", "replay", "log"],
        vec!["git", "bisect", "fixed"],
        vec!["git", "checkout", "target"],
    ] {
        assert!(capture(&args, frames()).receipt(0).is_none(), "{args:?}");
    }
}

fn parent_frames() -> Vec<Value> {
    vec![
        json!({"event":"child_start", "sid":"root", "child_id":0, "use_shell":false, "argv":["git", "checkout", "-q", "target", "--"]}),
        json!({"event":"child_exit", "sid":"root", "child_id":0, "code":0}),
    ]
}

#[test]
fn bisect_capture_parent_receipt_survives_a_delayed_child_stream() {
    for prefix in 0..=frames().len() {
        let mut events = parent_frames();
        events.splice(1..1, frames().into_iter().take(prefix));
        let capture = capture(&["git", "bisect", "good"], events);
        let receipt = capture.receipt(0).unwrap();
        assert_eq!(receipt.target, "target");
        assert_eq!(receipt.started_at_ns, 100);
        assert_eq!(receipt.finished_at_ns, 101 + prefix as u128);
        assert!(capture.receipt(1).is_none());
    }
}

#[test]
fn bisect_capture_rejects_unproven_parent_children() {
    for boundary in [
        "missing_start",
        "missing_exit",
        "missing_id",
        "wrong_id",
        "failed",
        "shell",
        "unknown_shell",
        "directory",
        "extra_args",
        "other_program",
        "two",
        "duplicate_exit",
    ] {
        let mut events = parent_frames();
        match boundary {
            "missing_start" => {
                events.remove(0);
            }
            "missing_exit" => {
                events.remove(1);
            }
            "missing_id" => {
                events[0].as_object_mut().unwrap().remove("child_id");
            }
            "wrong_id" => events[1]["child_id"] = json!(1),
            "failed" => events[1]["code"] = json!(1),
            "shell" => events[0]["use_shell"] = json!(true),
            "unknown_shell" => {
                events[0].as_object_mut().unwrap().remove("use_shell");
            }
            "directory" => events[0]["cd"] = json!("/other"),
            "extra_args" => {
                events[0]["argv"] = json!(["git", "checkout", "-q", "target", "--", "file"])
            }
            "other_program" => events[0]["argv"][0] = json!("custom-git"),
            "two" => events.extend(parent_frames()),
            "duplicate_exit" => events.push(events[1].clone()),
            _ => unreachable!(),
        }
        assert!(
            capture(&["git", "bisect", "good"], events)
                .receipt(0)
                .is_none(),
            "{boundary}"
        );
    }
}

#[test]
fn bisect_capture_parent_receipt_does_not_override_conflicting_child_evidence() {
    for boundary in ["foreign", "target", "failed", "name", "nested"] {
        let mut child = frames();
        match boundary {
            "foreign" => child[1]["worktree"] = json!("/other"),
            "target" => child[0]["argv"][3] = json!("other"),
            "failed" => child[3]["code"] = json!(1),
            "name" => child[2]["name"] = json!("reset"),
            "nested" => child[0]["sid"] = json!("root/child/nested"),
            _ => unreachable!(),
        }
        let mut events = parent_frames();
        events.splice(1..1, child);
        assert!(
            capture(&["git", "bisect", "good"], events)
                .receipt(0)
                .is_none(),
            "{boundary}"
        );
    }
}
