use super::*;
use serde_json::json;

fn start(waits: &mut CommitEditorWaits, command: &str) {
    waits.observe(&json!({"event":"start","sid":"root","argv":["git",command]}));
}

fn editor(waits: &mut CommitEditorWaits) -> Option<String> {
    waits.observe(&json!({"event":"child_start","sid":"root","child_class":"editor","child_id":7} ))
}

#[test]
fn only_a_root_commit_editor_with_an_identified_child_yields() {
    for command in ["commit", "rebase", "merge", "cherry-pick"] {
        let mut waits = CommitEditorWaits::default();
        start(&mut waits, command);
        assert_eq!(editor(&mut waits).is_some(), command == "commit");
    }
    let mut waits = CommitEditorWaits::default();
    start(&mut waits, "commit");
    waits.observe(
        &json!({"event":"child_start","sid":"root/child","child_class":"editor","child_id":7}),
    );
    assert!(!waits.is_waiting("root"));
    waits.observe(&json!({"event":"child_start","sid":"root","child_class":"editor"}));
    assert!(!waits.is_waiting("root"));
}

#[test]
fn only_the_matching_editor_exit_restores_the_barrier() {
    let mut waits = CommitEditorWaits::default();
    start(&mut waits, "commit");
    assert_eq!(editor(&mut waits).as_deref(), Some("root"));
    waits.observe(&json!({"event":"child_exit","sid":"root","child_id":8}));
    assert!(waits.is_waiting("root"));
    waits.observe(&json!({"event":"child_exit","sid":"root","child_id":7}));
    assert!(!waits.is_waiting("root"));
}

#[test]
fn nested_mutations_before_or_during_the_editor_prevent_yielding() {
    for before_root in [false, true] {
        let mut waits = CommitEditorWaits::default();
        if !before_root {
            start(&mut waits, "commit");
            assert!(editor(&mut waits).is_some());
        }
        waits.observe(&json!({"event":"start","sid":"root/child","argv":["git","update-ref","refs/heads/main","HEAD"]}));
        if before_root {
            start(&mut waits, "commit");
        }
        assert!(!waits.is_waiting("root"));
        assert!(editor(&mut waits).is_none());
    }
}

#[test]
fn read_only_nested_git_does_not_end_the_commit_editor_wait() {
    let mut waits = CommitEditorWaits::default();
    start(&mut waits, "commit");
    editor(&mut waits);
    waits.observe(&json!({"event":"start","sid":"root/child","argv":["git","status"]}));
    assert!(waits.is_waiting("root"));
}

#[test]
fn an_unclassified_nested_start_restores_the_barrier() {
    let mut waits = CommitEditorWaits::default();
    start(&mut waits, "commit");
    editor(&mut waits);
    waits.observe(&json!({"event":"start","sid":"root/child","argv":[]}));
    assert!(!waits.is_waiting("root"));
    assert!(editor(&mut waits).is_none());
}

#[test]
fn overlapping_or_unidentified_editors_keep_the_root_ordered() {
    for child_id in [json!(8), Value::Null] {
        let mut waits = CommitEditorWaits::default();
        start(&mut waits, "commit");
        editor(&mut waits);
        waits.observe(
            &json!({"event":"child_start","sid":"root","child_class":"editor","child_id":child_id}),
        );
        assert!(!waits.is_waiting("root"));
        waits.observe(&json!({"event":"child_exit","sid":"root","child_id":7}));
        assert!(editor(&mut waits).is_none());
    }
}

#[test]
fn parent_exit_restores_the_barrier_and_atexit_clears_state() {
    for event in ["exit", "atexit", "signal"] {
        let mut waits = CommitEditorWaits::default();
        start(&mut waits, "commit");
        editor(&mut waits);
        waits.observe(&json!({"event":event,"sid":"root"}));
        assert!(!waits.is_waiting("root"), "{event}");
        if event == "atexit" {
            assert!(waits.roots.is_empty());
        }
    }
}
