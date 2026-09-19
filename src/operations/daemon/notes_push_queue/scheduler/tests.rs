use super::*;

fn request(id: usize, destinations: &[&str]) -> Request<usize> {
    Request {
        payload: id,
        destinations: destinations.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn active_pass_cannot_acknowledge_newer_push_and_pending_targets_coalesce() {
    let mut queue = Scheduler::default();
    assert!(
        queue
            .admit("a", Path::new("root"), request(1, &["one"]))
            .is_ok()
    );
    let active = queue.take_ready().unwrap();
    assert!(
        queue
            .admit("a", Path::new("root"), request(2, &["one", "two"]))
            .is_ok()
    );
    assert!(
        queue
            .admit("a", Path::new("root"), request(3, &["two"]))
            .is_ok()
    );
    assert!(queue.take_ready().is_none());
    queue.complete("a", active.requests.len());
    let pending = queue.take_ready().unwrap();
    assert_eq!(pending.destinations, ["one", "two"]);
    assert_eq!(
        pending
            .requests
            .iter()
            .map(|r| r.payload)
            .collect::<Vec<_>>(),
        [2, 3]
    );
    queue.complete("a", pending.requests.len());
    assert_eq!(queue.outstanding, 0);
}

#[test]
fn active_family_limit_does_not_starve_another_ready_family() {
    let mut queue = Scheduler::default();
    for id in 0..MAX_ACTIVE_FAMILIES + 1 {
        assert!(
            queue
                .admit(&id.to_string(), Path::new("root"), request(id, &["one"]))
                .is_ok()
        );
    }
    for _ in 0..MAX_ACTIVE_FAMILIES {
        assert!(queue.take_ready().is_some());
    }
    assert!(queue.take_ready().is_none());
    assert!(
        queue
            .admit("0", Path::new("root"), request(99, &["two"]))
            .is_ok()
    );
    queue.complete("1", 1);
    assert_eq!(
        queue.take_ready().unwrap().family,
        MAX_ACTIVE_FAMILIES.to_string()
    );
}

#[test]
fn request_limit_includes_active_work_and_returns_rejected_payload() {
    let mut queue = Scheduler::default();
    assert!(
        queue
            .admit("a", Path::new("root"), request(0, &["one"]))
            .is_ok()
    );
    let active = queue.take_ready().unwrap();
    for id in 1..MAX_REQUESTS {
        assert!(
            queue
                .admit("a", Path::new("root"), request(id, &["one"]))
                .is_ok()
        );
    }
    assert_eq!(
        queue
            .admit("b", Path::new("root"), request(999, &["one"]))
            .unwrap_err()
            .payload,
        999
    );
    queue.complete("a", active.requests.len());
    assert!(
        queue
            .admit("b", Path::new("root"), request(999, &["one"]))
            .is_ok()
    );
}

#[test]
fn destination_overflow_preserves_every_previously_admitted_request() {
    let mut queue = Scheduler::default();
    for id in 0..MAX_DESTINATIONS {
        assert!(
            queue
                .admit("a", Path::new("root"), request(id, &[&id.to_string()]))
                .is_ok()
        );
    }
    assert!(
        queue
            .admit("a", Path::new("root"), request(99, &["extra"]))
            .is_err()
    );
    assert!(
        queue
            .admit("a", Path::new("root"), request(100, &["0"]))
            .is_ok()
    );
    let batch = queue.take_ready().unwrap();
    assert_eq!(batch.destinations.len(), MAX_DESTINATIONS);
    assert_eq!(batch.requests.len(), MAX_DESTINATIONS + 1);
    assert_eq!(batch.requests.last().unwrap().payload, 100);
}

#[test]
fn worktree_contexts_do_not_coalesce_but_still_serialize_within_the_family() {
    let mut queue = Scheduler::default();
    assert!(
        queue
            .admit("a", Path::new("one"), request(1, &["remote"]))
            .is_ok()
    );
    assert!(
        queue
            .admit("a", Path::new("two"), request(2, &["remote"]))
            .is_ok()
    );
    let first = queue.take_ready().unwrap();
    assert_eq!(first.requests.len(), 1);
    assert!(queue.take_ready().is_none());
    queue.complete("a", 1);
    let second = queue.take_ready().unwrap();
    assert_eq!(second.context, Path::new("two"));
    assert_eq!(second.requests[0].payload, 2);
}
