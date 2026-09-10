use super::*;

#[test]
fn jj_observer_intent_empty_persisted_diagnostic_loads_and_replaces() {
    let fixture = Fixture::new();
    let connection = fixture.manual(DDL, 1, true);
    let mut first = record(1);
    first.blocked = Some(JjObserverError {
        code: "unavailable".to_owned(),
        message: String::new(),
        persisted: true,
    });
    raw_insert(&connection, 1, &encoded(&first));
    assert_loaded(&fixture.path, &first);
    let mut next = record(2);
    next.blocked = Some(JjObserverError {
        code: "still_unavailable".to_owned(),
        message: String::new(),
        persisted: true,
    });
    replace(&fixture.path, &Some(first), &next).unwrap();
    assert_loaded(&fixture.path, &next);

    let fresh = Fixture::new();
    next.revision = 1;
    replace(&fresh.path, &None, &next).unwrap();
    assert_loaded(&fresh.path, &next);
}
