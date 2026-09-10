use super::support::Harness;
use super::*;

#[test]
fn jj_daemon_observer_enable_admits_and_isolates_separate_daemon_homes() {
    let mut h = Harness::new(true);
    let a = native::rich_parent();
    h.select(&a);
    h.start();
    let saved = h.saved_rows();
    let files = manifest(h.fixture.repo.path());
    let enabled = h.ready_control("enable");
    assert_eq!(enabled["disposition"], "enabled");
    let active = h.active(1, std::slice::from_ref(&a.operation_id));
    assert_eq!(active["target"], enabled["target"]);
    let packet = h.packet(1);
    assert_eq!(packet.ordered_operations(), std::slice::from_ref(&a));
    assert_eq!(packet.receipt().expected_generation(), 0);
    assert_eq!(packet.reached_baseline_ids(), h.baseline_heads());
    let rows = h.admission_rows();
    let repeated = h.ready_control("enable");
    assert_eq!(repeated["disposition"], "already_enabled");
    assert_eq!(repeated["revision"], enabled["revision"]);
    assert_eq!(repeated["session_cursor"], active["session_cursor"]);
    assert_eq!(h.admission_rows(), rows);
    assert_eq!(h.saved_rows(), saved);
    assert_eq!(manifest(h.fixture.repo.path()), files);

    let mut other = Harness::new(false);
    other.start();
    let status = other.control("status", true);
    assert_eq!(status["runtime"], "disabled");
    assert_eq!(status["revision"], 0);
    assert!(status["target"].is_null());
    assert_eq!(other.packet_count(), 0);
    h.disabled();
}

#[test]
fn jj_daemon_observer_restart_uses_current_sql_progress_and_original_cutoff() {
    let mut h = Harness::new(false);
    let a = native::rich_parent();
    let b = native::rich_child();
    h.fixture.write_evidence(&b);
    h.select(&a);
    h.start();
    h.ready_control("enable");
    let first = h.active(1, std::slice::from_ref(&a.operation_id));
    let saved = h.saved_rows();
    h.fixture.repo.shutdown_dedicated_daemon_for_test();
    h.select(&b);
    h.manual_capture(1, std::slice::from_ref(&a.operation_id));
    assert_eq!(h.packet(2).ordered_operations(), [a.clone(), b.clone()]);
    h.fixture.repo.restart_dedicated_daemon_for_test();
    let resumed = h.active(2, std::slice::from_ref(&b.operation_id));
    assert_eq!(resumed["revision"], first["revision"]);
    assert_eq!(resumed["target"], first["target"]);
    assert_eq!(h.packet_count(), 2);
    assert_eq!(h.saved_rows(), saved);
    h.disabled();
}

#[test]
fn jj_daemon_observer_manual_same_head_capture_blocks_until_explicit_resume() {
    let mut h = Harness::new(true);
    h.start();
    let saved = h.saved_rows();
    let baseline = h.baseline_heads();
    h.ready_control("enable");
    let original = h.active(0, &baseline);
    // H stays fixed, so no observer attempt can legitimately append before this write.
    h.manual_capture(0, &baseline);
    let manual = h.packet(1);
    assert!(manual.ordered_operations().is_empty());
    let blocked = h.blocked("admission_unavailable");
    assert_eq!(blocked["session_cursor"], original["session_cursor"]);
    assert!(blocked["revision"].as_u64().unwrap() > original["revision"].as_u64().unwrap());
    assert_eq!(h.packet_count(), 1);
    let duplicate = h.control("enable", false);
    assert_eq!(duplicate["error"]["code"], "resume_required");
    assert_eq!(duplicate["revision"], blocked["revision"]);
    h.fixture.repo.restart_dedicated_daemon_for_test();
    let reopened = h.blocked("admission_unavailable");
    assert_eq!(reopened["revision"], blocked["revision"]);
    assert_eq!(h.packet_count(), 1);
    let resume = h.ready_control("resume");
    assert_eq!(resume["disposition"], "resume_started");
    h.active(1, &baseline);
    // A second manual advance proves the resumed background session continues
    // after its client exits, without manufacturing repeated-head packets itself.
    h.manual_capture(1, &baseline);
    h.blocked("admission_unavailable");
    assert_eq!(h.packet_count(), 2);
    assert_eq!(h.packet(2).receipt().expected_generation(), 1);
    assert_eq!(h.saved_rows(), saved);
    h.disabled();
}

#[test]
fn jj_daemon_observer_disable_drains_and_stays_disabled_after_restart() {
    let mut h = Harness::new(true);
    h.start();
    h.ready_control("enable");
    h.active(0, &h.baseline_heads());
    let disabled = h.disabled();
    let again = h.control("disable", true);
    assert_eq!(again["disposition"], "already_disabled");
    assert_eq!(again["revision"], disabled["revision"]);
    assert_eq!(again["in_flight"], false);
    let a = native::rich_parent();
    h.select(&a);
    let unchanged = h.admission_rows();
    assert_eq!(h.control("status", true)["runtime"], "disabled");
    assert_eq!(h.packet_count(), 0);
    h.fixture.repo.restart_dedicated_daemon_for_test();
    let stopped = h.wait(|v| v["runtime"] == "disabled");
    assert_eq!(stopped["revision"], disabled["revision"]);
    assert_eq!(stopped["target"], h.target());
    assert_eq!(h.admission_rows(), unchanged);
    let resume = h.ready_control("resume");
    assert_eq!(resume["disposition"], "resume_started");
    h.active(1, std::slice::from_ref(&a.operation_id));
    assert_eq!(h.packet(1).ordered_operations(), [a]);
    h.disabled();
}
