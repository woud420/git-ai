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
