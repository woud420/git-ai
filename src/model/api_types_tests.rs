use super::*;
use crate::model::authorship_log::LineRange;
use crate::model::diff_json::FileDiffJson;
use std::collections::BTreeMap;

#[test]
fn test_api_file_record_from_file_diff_empty() {
    let file_diff = FileDiffJson {
        annotations: BTreeMap::new(),
        diff: "".to_string(),
        base_content: "".to_string(),
    };

    let api_record = ApiFileRecord::from(&file_diff);
    assert_eq!(api_record.annotations.len(), 0);
    assert_eq!(api_record.diff, "");
    assert_eq!(api_record.base_content, "");
}

#[test]
fn test_api_file_record_from_file_diff_single_lines() {
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "prompt_hash_1".to_string(),
        vec![LineRange::Single(5), LineRange::Single(10)],
    );

    let file_diff = FileDiffJson {
        annotations,
        diff: "diff content".to_string(),
        base_content: "base content".to_string(),
    };

    let api_record = ApiFileRecord::from(&file_diff);
    assert_eq!(api_record.annotations.len(), 1);

    let ranges = &api_record.annotations["prompt_hash_1"];
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0], serde_json::Value::Number(5.into()));
    assert_eq!(ranges[1], serde_json::Value::Number(10.into()));
    assert_eq!(api_record.diff, "diff content");
    assert_eq!(api_record.base_content, "base content");
}

#[test]
fn test_api_file_record_from_file_diff_ranges() {
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "prompt_hash_2".to_string(),
        vec![LineRange::Range(1, 5), LineRange::Range(10, 15)],
    );

    let file_diff = FileDiffJson {
        annotations,
        diff: "diff".to_string(),
        base_content: "base".to_string(),
    };

    let api_record = ApiFileRecord::from(&file_diff);
    let ranges = &api_record.annotations["prompt_hash_2"];
    assert_eq!(ranges.len(), 2);

    match &ranges[0] {
        serde_json::Value::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], serde_json::Value::Number(1.into()));
            assert_eq!(arr[1], serde_json::Value::Number(5.into()));
        }
        _ => panic!("Expected array"),
    }

    match &ranges[1] {
        serde_json::Value::Array(arr) => {
            assert_eq!(arr.len(), 2);
            assert_eq!(arr[0], serde_json::Value::Number(10.into()));
            assert_eq!(arr[1], serde_json::Value::Number(15.into()));
        }
        _ => panic!("Expected array"),
    }
}

#[test]
fn test_api_file_record_from_file_diff_mixed() {
    let mut annotations = BTreeMap::new();
    annotations.insert(
        "prompt_hash".to_string(),
        vec![
            LineRange::Single(1),
            LineRange::Range(5, 10),
            LineRange::Single(20),
        ],
    );

    let file_diff = FileDiffJson {
        annotations,
        diff: String::new(),
        base_content: String::new(),
    };

    let api_record = ApiFileRecord::from(&file_diff);
    let ranges = &api_record.annotations["prompt_hash"];
    assert_eq!(ranges.len(), 3);
    assert_eq!(ranges[0], serde_json::Value::Number(1.into()));

    match &ranges[1] {
        serde_json::Value::Array(arr) => {
            assert_eq!(arr[0], serde_json::Value::Number(5.into()));
            assert_eq!(arr[1], serde_json::Value::Number(10.into()));
        }
        _ => panic!("Expected array"),
    }

    assert_eq!(ranges[2], serde_json::Value::Number(20.into()));
}

#[test]
fn test_create_bundle_response_deserialization() {
    let json = r#"{
            "success": true,
            "id": "bundle123",
            "url": "https://example.com/bundle123"
        }"#;

    let response: CreateBundleResponse = serde_json::from_str(json).unwrap();
    assert!(response.success);
    assert_eq!(response.id, "bundle123");
    assert_eq!(response.url, "https://example.com/bundle123");
}

#[test]
fn test_api_error_response_serialization() {
    let error = ApiErrorResponse {
        error: "Invalid request".to_string(),
        details: Some(serde_json::json!({"field": "title"})),
    };

    let json = serde_json::to_string(&error).unwrap();
    assert!(json.contains("Invalid request"));
    assert!(json.contains("field"));
}

#[test]
fn test_api_error_response_without_details() {
    let error = ApiErrorResponse {
        error: "Error".to_string(),
        details: None,
    };

    let json = serde_json::to_string(&error).unwrap();
    assert!(json.contains("Error"));
    assert!(!json.contains("details"));
}

#[test]
fn test_cas_object_serialization() {
    let mut metadata = HashMap::new();
    metadata.insert("key1".to_string(), "value1".to_string());

    let cas_object = CasObject {
        content: serde_json::json!({"data": "test"}),
        hash: "abc123".to_string(),
        metadata,
    };

    let json = serde_json::to_string(&cas_object).unwrap();
    assert!(json.contains("abc123"));
    assert!(json.contains("key1"));
}

#[test]
fn test_cas_object_empty_metadata() {
    let cas_object = CasObject {
        content: serde_json::json!({}),
        hash: "hash".to_string(),
        metadata: HashMap::new(),
    };

    let json = serde_json::to_string(&cas_object).unwrap();
    assert!(!json.contains("metadata"));
}

#[test]
fn test_cas_upload_request() {
    let objects = vec![
        CasObject {
            content: serde_json::json!({"test": 1}),
            hash: "h1".to_string(),
            metadata: HashMap::new(),
        },
        CasObject {
            content: serde_json::json!({"test": 2}),
            hash: "h2".to_string(),
            metadata: HashMap::new(),
        },
    ];

    let request = CasUploadRequest { objects };
    let json = serde_json::to_string(&request).unwrap();
    assert!(json.contains("h1"));
    assert!(json.contains("h2"));
}

#[test]
fn test_cas_upload_result() {
    let result = CasUploadResult {
        hash: "hash1".to_string(),
        status: "ok".to_string(),
        error: None,
    };

    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("ok"));
    assert!(!json.contains("error"));
}

#[test]
fn test_cas_upload_result_with_error() {
    let result = CasUploadResult {
        hash: "hash2".to_string(),
        status: "error".to_string(),
        error: Some("Upload failed".to_string()),
    };

    let json = serde_json::to_string(&result).unwrap();
    assert!(json.contains("error"));
    assert!(json.contains("Upload failed"));
}

#[test]
fn test_cas_upload_response() {
    let response = CasUploadResponse {
        results: vec![
            CasUploadResult {
                hash: "h1".to_string(),
                status: "ok".to_string(),
                error: None,
            },
            CasUploadResult {
                hash: "h2".to_string(),
                status: "error".to_string(),
                error: Some("Failed".to_string()),
            },
        ],
        success_count: 1,
        failure_count: 1,
    };

    let json = serde_json::to_string(&response).unwrap();
    assert!(json.contains("success_count"));
    assert!(json.contains("failure_count"));
}

#[test]
fn test_api_file_record_clone() {
    let record = ApiFileRecord {
        annotations: HashMap::new(),
        diff: "test".to_string(),
        base_content: "base".to_string(),
    };

    let cloned = record.clone();
    assert_eq!(record, cloned);
}

#[test]
fn test_cas_messages_object() {
    use crate::model::transcript::Message;

    let messages = vec![Message::user("test".to_string(), None)];

    let cas_msg = CasMessagesObject {
        messages: messages.clone(),
    };

    let json = serde_json::to_string(&cas_msg).unwrap();
    assert!(json.contains("test"));

    let deserialized: CasMessagesObject = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.messages.len(), 1);
}

#[test]
fn daemon_logs_upload_request_serializes_contract_fields() {
    let mut fields = BTreeMap::new();
    fields.insert(
        "uptime_seconds".to_string(),
        DaemonLogFieldValue::from(900_u64),
    );
    fields.insert("healthy".to_string(), DaemonLogFieldValue::from(true));

    let request = DaemonLogsUploadRequest {
        version: DAEMON_LOGS_UPLOAD_VERSION,
        git_ai_version: Some("1.2.3".to_string()),
        daemon_id: Some("daemon-1".to_string()),
        install_id: Some("install-1".to_string()),
        repo_url: None,
        events: vec![DaemonLogEvent {
            id: Some("event-1".to_string()),
            kind: DaemonLogKind::Heartbeat,
            timestamp: "2026-06-26T12:00:00.000Z".to_string(),
            level: DaemonLogLevel::Info,
            target: Some("git_ai::daemon".to_string()),
            message: "alive".to_string(),
            fields,
            repo_url: None,
            git_ai_version: None,
        }],
    };

    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["version"], 1);
    assert_eq!(value["git_ai_version"], "1.2.3");
    assert_eq!(value["daemon_id"], "daemon-1");
    assert_eq!(value["events"][0]["kind"], "heartbeat");
    assert_eq!(value["events"][0]["level"], "info");
    assert_eq!(value["events"][0]["fields"]["uptime_seconds"], 900);
    assert_eq!(value["events"][0]["fields"]["healthy"], true);
}

#[test]
fn daemon_logs_upload_response_accepts_null_error_index() {
    let response: DaemonLogsUploadResponse = serde_json::from_str(
        r#"{"accepted":0,"dropped":0,"enqueued":false,"errors":[{"index":null,"error":"bad"}]}"#,
    )
    .unwrap();

    assert_eq!(response.errors[0].index, None);
}
