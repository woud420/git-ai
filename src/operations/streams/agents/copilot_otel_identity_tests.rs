#[test]
fn test_extract_event_session_id_chat_session_id() {
    use crate::operations::streams::agent::Agent;
    use crate::operations::streams::agents::CopilotAgent;

    let agent = CopilotAgent::new();
    let event = serde_json::json!({
        "span": {
            "chat_session_id": "chat-sess-123",
            "conversation_id": "conv-456",
        },
        "attributes": {},
        "events": [],
    });
    // Prefers chat_session_id over conversation_id
    assert_eq!(
        agent.extract_event_session_id(&event),
        Some("chat-sess-123".to_string())
    );
}

#[test]
fn test_extract_event_session_id_fallback_to_conversation_id() {
    use crate::operations::streams::agent::Agent;
    use crate::operations::streams::agents::CopilotAgent;

    let agent = CopilotAgent::new();
    let event = serde_json::json!({
        "span": {
            "chat_session_id": null,
            "conversation_id": "conv-789",
        },
        "attributes": {},
        "events": [],
    });
    assert_eq!(
        agent.extract_event_session_id(&event),
        Some("conv-789".to_string())
    );
}

#[test]
fn test_extract_event_session_id_empty_strings_return_none() {
    use crate::operations::streams::agent::Agent;
    use crate::operations::streams::agents::CopilotAgent;

    let agent = CopilotAgent::new();
    let event = serde_json::json!({
        "span": {
            "chat_session_id": "",
            "conversation_id": "",
        },
        "attributes": {},
        "events": [],
    });
    assert_eq!(agent.extract_event_session_id(&event), None);
}

#[test]
fn test_extract_event_session_id_no_span_key() {
    use crate::operations::streams::agent::Agent;
    use crate::operations::streams::agents::CopilotAgent;

    let agent = CopilotAgent::new();
    let event = serde_json::json!({"type": "user", "content": "hello"});
    assert_eq!(agent.extract_event_session_id(&event), None);
}

#[test]
fn test_extract_event_session_id_missing_both_fields() {
    use crate::operations::streams::agent::Agent;
    use crate::operations::streams::agents::CopilotAgent;

    let agent = CopilotAgent::new();
    let event = serde_json::json!({
        "span": {
            "span_id": "abc",
            "trace_id": "t1",
        },
        "attributes": {},
        "events": [],
    });
    assert_eq!(agent.extract_event_session_id(&event), None);
}

#[test]
fn test_extract_event_session_id_empty_chat_falls_to_conversation() {
    use crate::operations::streams::agent::Agent;
    use crate::operations::streams::agents::CopilotAgent;

    let agent = CopilotAgent::new();
    let event = serde_json::json!({
        "span": {
            "chat_session_id": "",
            "conversation_id": "conv-fallback",
        },
        "attributes": {},
        "events": [],
    });
    assert_eq!(
        agent.extract_event_session_id(&event),
        Some("conv-fallback".to_string())
    );
}
