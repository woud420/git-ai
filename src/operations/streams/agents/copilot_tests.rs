use super::*;
use crate::model::stream_watermark::{ByteOffsetWatermark, RecordIndexWatermark};
use crate::operations::streams::agents::test_support::{
    StreamAdapterContractCapabilities, assert_stream_adapter_contract, jsonl_fixture,
    rewritten_file_fixture,
};

#[test]
fn test_sweep_strategy() {
    let agent = CopilotAgent::new();
    assert_eq!(
        agent.sweep_strategy(),
        SweepStrategy::Periodic(Duration::from_secs(30 * 60))
    );
}

#[test]
fn test_determine_format() {
    let json_path = PathBuf::from("/path/to/session.json");
    assert_eq!(
        CopilotAgent::determine_format(&json_path),
        StreamFormat::CopilotSessionJson
    );

    let jsonl_path = PathBuf::from("/path/to/events.jsonl");
    assert_eq!(
        CopilotAgent::determine_format(&jsonl_path),
        StreamFormat::CopilotEventStreamJsonl
    );
}

// -- Event stream (JSONL / ByteOffset) batch-resume tests --

fn make_event_stream_line(i: usize) -> String {
    format!(
        r#"{{"type":"user.message","id":{},"data":{{"content":"msg-{}"}},"timestamp":"2025-01-01T00:00:{:02}Z"}}"#,
        i, i, i
    )
}

#[test]
fn test_event_stream_contract() {
    use tempfile::NamedTempFile;

    let file = NamedTempFile::with_suffix(".jsonl").unwrap();
    let mut fixture = jsonl_fixture(file.path(), make_event_stream_line);
    let agent = CopilotAgent::with_batch_size(2);
    assert_stream_adapter_contract(
        &agent,
        &mut fixture,
        || Box::new(ByteOffsetWatermark::new(0)),
        |event| event["id"].as_u64().unwrap().to_string(),
        2,
        "test",
        StreamAdapterContractCapabilities::APPEND_ALL,
    );
}

// -- Session JSON (RecordIndex) batch-resume tests --

fn make_session_json(request_count: usize) -> String {
    let requests: Vec<String> = (0..request_count)
            .map(|i| {
                format!(
                    r#"{{"id":{},"message":{{"text":"msg-{}"}},"response":[{{"kind":"markdownContent","value":"reply-{}"}}]}}"#,
                    i, i, i
                )
            })
            .collect();
    format!(r#"{{"requests":[{}]}}"#, requests.join(","))
}

#[test]
fn test_session_json_contract() {
    use tempfile::NamedTempFile;

    let file = NamedTempFile::with_suffix(".json").unwrap();
    let mut fixture = rewritten_file_fixture(file.path(), make_session_json);
    let agent = CopilotAgent::with_batch_size(2);
    assert_stream_adapter_contract(
        &agent,
        &mut fixture,
        || Box::new(RecordIndexWatermark::new(0)),
        |event| event["id"].as_u64().unwrap().to_string(),
        2,
        "test",
        StreamAdapterContractCapabilities::APPEND_ALL,
    );
}

#[test]
fn test_read_session_json_basic() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    let mut file = NamedTempFile::new().unwrap();
    let json = r#"{
            "requests": [
                {
                    "timestamp": 1704067200000,
                    "message": {"text": "Hello"},
                    "response": [
                        {"kind": "markdownContent", "value": "Hi there"}
                    ]
                }
            ],
            "inputState": {
                "selectedModel": {"identifier": "copilot/gpt-4"}
            }
        }"#;
    write!(file, "{}", json).unwrap();
    file.flush().unwrap();

    let agent = CopilotAgent::new();
    let watermark = Box::new(RecordIndexWatermark::new(0));
    let result = agent
        .read_incremental(file.path(), watermark, "test-session")
        .unwrap();

    // Each request object is returned as a raw JSON event
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0]["message"]["text"], "Hello");
    assert_eq!(result.events[0]["response"][0]["kind"], "markdownContent");
}

#[test]
fn test_read_event_stream_basic() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a .jsonl file
    let mut file = NamedTempFile::with_suffix(".jsonl").unwrap();
    writeln!(
            file,
            r#"{{"type":"user.message","data":{{"content":"Hello"}},"timestamp":"2025-01-01T00:00:00Z"}}"#
        )
        .unwrap();
    writeln!(
            file,
            r#"{{"type":"assistant.message","data":{{"content":"Hi there","modelId":"copilot/gpt-4"}},"timestamp":"2025-01-01T00:00:01Z"}}"#
        )
        .unwrap();
    file.flush().unwrap();

    let agent = CopilotAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let result = agent
        .read_incremental(file.path(), watermark, "test-session")
        .unwrap();

    // Both JSONL lines are returned as raw JSON
    assert_eq!(result.events.len(), 2);
    assert_eq!(result.events[0]["type"], "user.message");
    assert_eq!(result.events[0]["data"]["content"], "Hello");
    assert_eq!(result.events[1]["type"], "assistant.message");
    assert_eq!(result.events[1]["data"]["modelId"], "copilot/gpt-4");
}

#[test]
fn test_extract_event_ids() {
    let agent = CopilotAgent::new();
    let event: serde_json::Value = serde_json::from_str(
            r#"{"type":"user.message","id":"ev-123","parentId":"ev-000","timestamp":"2026-05-11T00:00:00Z"}"#,
        )
        .unwrap();
    let (id, parent_id, third) = agent.extract_event_ids(&event);
    assert_eq!(id, Some("ev-123".to_string()));
    assert_eq!(parent_id, Some("ev-000".to_string()));
    assert_eq!(third, None);
}

#[test]
fn test_extract_event_ids_null_parent() {
    let agent = CopilotAgent::new();
    let event: serde_json::Value =
        serde_json::from_str(r#"{"type":"session.start","id":"ev-001","parentId":null}"#).unwrap();
    let (id, parent_id, _) = agent.extract_event_ids(&event);
    assert_eq!(id, Some("ev-001".to_string()));
    assert_eq!(parent_id, None);
}

#[test]
fn test_infer_cwd_from_workspace_json() {
    use std::path::PathBuf;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/copilot_vscode_workspace/GitHub.copilot-chat/transcripts/test-session-abc.jsonl");
    let result = CopilotAgent::infer_cwd_from_workspace_json(&fixture);
    assert_eq!(result, Some(PathBuf::from("/Users/test/project")));
}

#[test]
fn test_infer_cwd_trait_method() {
    use std::path::PathBuf;

    let agent = CopilotAgent::new();
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/copilot_vscode_workspace/GitHub.copilot-chat/transcripts/test-session-abc.jsonl");
    let result = agent.infer_cwd(&fixture);
    // workspace.json says file:///Users/test/project
    assert_eq!(result, Some(PathBuf::from("/Users/test/project")));
}

#[test]
fn test_percent_decode_path() {
    assert_eq!(
        percent_decode_path("/Users/test%20user/my%20project"),
        "/Users/test user/my project"
    );
    assert_eq!(percent_decode_path("/normal/path"), "/normal/path");
    assert_eq!(
        percent_decode_path("/path%2Fwith%2Fencoded"),
        "/path/with/encoded"
    );
    // Multi-byte UTF-8: é = %C3%A9
    assert_eq!(percent_decode_path("/caf%C3%A9/project"), "/café/project");
}

#[test]
fn test_read_vscode_event_stream_fixture() {
    use std::path::PathBuf;

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/copilot_vscode_event_stream.jsonl");
    let agent = CopilotAgent::new();
    let watermark = Box::new(ByteOffsetWatermark::new(0));
    let result = agent
        .read_incremental(&fixture, watermark, "test-session")
        .unwrap();

    assert_eq!(result.events.len(), 13);
    assert_eq!(result.events[0]["type"], "session.start");
    assert_eq!(
        result.events[0]["data"]["sessionId"],
        "5fcddc54-3ba1-4fc4-88ac-86bd2ce74c19"
    );
    assert_eq!(result.events[1]["type"], "user.message");
    assert_eq!(result.events[6]["type"], "tool.execution_start");
    assert_eq!(result.events[6]["data"]["toolName"], "read_file");
}
