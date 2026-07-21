use super::test_support::*;
use super::*;

#[test]
fn test_session_event_candidates_near_timestamps_filters_kind_and_window() {
    let (mut db, _temp_dir) = create_test_db();
    let base_ts = seconds_ago(60);
    let events = vec![
        session_event_json(
            base_ts,
            "session-near",
            "external-near",
            "codex",
            Some("https://github.com/acme/repo"),
        ),
        session_event_json(
            base_ts + 10,
            "session-far",
            "external-far",
            "codex",
            Some("https://github.com/acme/repo"),
        ),
        format!(
            r#"{{
                "t":{base_ts},
                "e":4,
                "v":{{"7":"checkpoint-tool-use"}},
                "a":{{"20":"codex","23":"external-checkpoint","24":"session-checkpoint"}}
            }}"#
        ),
    ];
    db.insert_events(&events).unwrap();

    let timestamp_ns = (base_ts as u128 * 1_000_000_000) + 500_000_000;
    let candidates = db
        .session_event_candidates_near_timestamps(&[timestamp_ns], 3_000_000_000)
        .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].event_ts, base_ts);
    assert_eq!(candidates[0].session_id, "session-near");
    assert_eq!(candidates[0].external_session_id, "external-near");
}

#[test]
fn test_session_event_candidates_treat_event_ts_as_second_bucket() {
    let (mut db, _temp_dir) = create_test_db();
    let base_ts = seconds_ago(60);
    db.insert_events(&[session_event_json(
        base_ts,
        "session-bucket",
        "external-bucket",
        "codex",
        Some("https://github.com/acme/repo"),
    )])
    .unwrap();

    let timestamp_ns = base_ts as u128 * NS_PER_SECOND + 3_500_000_000;
    let candidates = db
        .session_event_candidates_near_timestamps(&[timestamp_ns], 3_000_000_000)
        .unwrap();

    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].session_id, "session-bucket");
}

#[test]
fn test_session_event_candidates_parse_required_and_optional_metadata() {
    let (mut db, _temp_dir) = create_test_db();
    let ts = seconds_ago(30);
    db.insert_events(&[
        session_event_json(
            ts,
            "session-complete",
            "external-complete",
            "claude-code",
            Some("https://github.com/acme/repo"),
        ),
        format!(
            r#"{{
                "t":{ts},
                "e":5,
                "v":{{"0":{{"type":"assistant"}}}},
                "a":{{"20":"codex","24":"missing-external-session"}}
            }}"#
        ),
    ])
    .unwrap();

    let timestamp_ns = ts as u128 * 1_000_000_000;
    let candidates = db
        .session_event_candidates_near_timestamps(&[timestamp_ns], 3_000_000_000)
        .unwrap();

    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert_eq!(candidate.session_id, "session-complete");
    assert_eq!(
        candidate.trace_id.as_deref(),
        Some("trace-session-complete")
    );
    assert_eq!(candidate.tool, "claude-code");
    assert_eq!(candidate.model.as_deref(), Some("gpt-5"));
    assert_eq!(candidate.external_session_id, "external-complete");
    assert_eq!(
        candidate.external_tool_use_id.as_deref(),
        Some("tool-use-session-complete")
    );
    assert_eq!(
        candidate.repo_url.as_deref(),
        Some("https://github.com/acme/repo")
    );
}
