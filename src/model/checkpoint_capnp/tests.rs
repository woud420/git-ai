use super::*;
use crate::checkpoint_capnp as wire;
use crate::model::checkpoint_request::{
    BaseCommit, CheckpointFile, CheckpointRequest, PreparedPathRole, StreamFormat, StreamSource,
};
use crate::model::working_log::{AgentId, CheckpointKind};
use std::collections::HashMap;

fn fixture() -> CheckpointDelivery {
    let repo = std::env::temp_dir().join("checkpoint-codec-fixture");
    CheckpointDelivery::from_requests_at(
        vec![CheckpointRequest {
            trace_id: "trace-α".into(),
            checkpoint_kind: CheckpointKind::AiAgent,
            agent_id: Some(AgentId {
                tool: "mock_ai".into(),
                id: "session-1".into(),
                model: "test-model".into(),
            }),
            files: vec![CheckpointFile {
                path: repo.join("source.rs"),
                content: Some("fn main() { println!(\"λ\\n\"); }\n".into()),
                repo_work_dir: repo.clone(),
                base_commit: BaseCommit::Sha("abc123".into()),
            }],
            path_role: PreparedPathRole::WillEdit,
            stream_source: Some(StreamSource {
                path: repo.join("transcript.jsonl"),
                format: StreamFormat::CodexJsonl,
                session_id: "internal-session".into(),
                external_session_id: "external-session".into(),
                external_parent_session_id: Some("parent-session".into()),
            }),
            metadata: HashMap::from([
                ("empty".into(), "".into()),
                ("unicode".into(), "line one\nline two\u{0000}雪".into()),
            ]),
            delivery_id: Some("request-delivery-id".into()),
        }],
        1_234_567_890,
    )
    .remove(0)
}

fn assert_round_trip(delivery: &CheckpointDelivery) {
    let bytes = encode_delivery(delivery).unwrap();
    let decoded = decode_delivery(&bytes).unwrap();
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(delivery).unwrap()
    );
}

#[test]
fn every_checkpoint_field_survives_typed_encoding() {
    assert_round_trip(&fixture());
}

#[test]
fn optional_fields_preserve_absent_and_empty_values() {
    for content in [None, Some(String::new())] {
        for parent in [None, Some(String::new())] {
            for delivery_id in [None, Some(String::new())] {
                let mut delivery = fixture();
                delivery.request.files[0].content = content.clone();
                delivery.request.files[0].base_commit = BaseCommit::Initial;
                delivery
                    .request
                    .stream_source
                    .as_mut()
                    .unwrap()
                    .external_parent_session_id = parent.clone();
                delivery.request.delivery_id = delivery_id;
                assert_round_trip(&delivery);
            }
        }
    }
    let mut delivery = fixture();
    delivery.request.agent_id = Some(AgentId {
        tool: String::new(),
        id: String::new(),
        model: String::new(),
    });
    let source = delivery.request.stream_source.as_mut().unwrap();
    source.session_id.clear();
    source.external_session_id.clear();
    source.external_parent_session_id = None;
    assert_round_trip(&delivery);
    delivery.request.agent_id = None;
    delivery.request.stream_source = None;
    delivery.request.files.clear();
    delivery.request.metadata.clear();
    delivery.request.path_role = PreparedPathRole::Edited;
    assert_round_trip(&delivery);
}

#[test]
fn every_checkpoint_and_transcript_kind_round_trips() {
    for kind in [
        CheckpointKind::Human,
        CheckpointKind::AiAgent,
        CheckpointKind::AiTab,
        CheckpointKind::KnownHuman,
    ] {
        for format in [
            StreamFormat::ClaudeJsonl,
            StreamFormat::ContinueJson,
            StreamFormat::GeminiJsonl,
            StreamFormat::WindsurfJsonl,
            StreamFormat::CodexJsonl,
            StreamFormat::CursorJsonl,
            StreamFormat::DroidJsonl,
            StreamFormat::CopilotSessionJson,
            StreamFormat::CopilotEventStreamJsonl,
            StreamFormat::AmpThreadJson,
            StreamFormat::OpenCodeSqlite,
            StreamFormat::PiJsonl,
        ] {
            let mut delivery = fixture();
            delivery.request.checkpoint_kind = kind;
            delivery.request.stream_source.as_mut().unwrap().format = format;
            assert_round_trip(&delivery);
        }
    }
}

#[test]
fn encode_enforces_the_existing_delivery_validation() {
    let mut delivery = fixture();
    delivery.schema_version += 1;
    assert!(encode_delivery(&delivery).is_err());
    delivery = fixture();
    delivery.request.files[0].path = "relative.rs".into();
    assert!(encode_delivery(&delivery).is_err());
    delivery = fixture();
    delivery.request.metadata = (0..=256)
        .map(|index| (format!("key-{index}"), "value".into()))
        .collect();
    assert!(encode_delivery(&delivery).is_err());
}

#[test]
fn decode_rejects_truncation_and_trailing_bytes() {
    let bytes = encode_delivery(&fixture()).unwrap();
    for length in [0, 1, 7, 8, bytes.len() / 2, bytes.len() - 1] {
        assert!(decode_delivery(&bytes[..length]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(&[0; 8]);
    assert!(decode_delivery(&trailing).is_err());
    trailing = bytes.clone();
    trailing.extend_from_slice(&bytes);
    assert!(decode_delivery(&trailing).is_err());
}

#[test]
fn unaligned_input_decodes_without_a_second_message_copy() {
    let delivery = fixture();
    let bytes = encode_delivery(&delivery).unwrap();
    let mut unaligned = vec![0];
    unaligned.extend_from_slice(&bytes);
    let decoded = decode_delivery(&unaligned[1..]).unwrap();
    assert_eq!(decoded.delivery_id, delivery.delivery_id);
}

#[test]
fn huge_segment_declarations_fail_without_allocating_the_claimed_segment() {
    let bytes = [0, 0, 0, 0, 255, 255, 255, 127];
    assert!(decode_delivery(&bytes).is_err());
}

fn modified_wire(change: impl FnOnce(wire::checkpoint_delivery::Builder<'_>)) -> Vec<u8> {
    let bytes = encode_delivery(&fixture()).unwrap();
    let source = capnp::serialize::read_message_from_flat_slice(
        &mut bytes.as_slice(),
        capnp::message::ReaderOptions::new(),
    )
    .unwrap();
    let mut message = capnp::message::Builder::new_default();
    message
        .set_root(
            source
                .get_root::<wire::checkpoint_delivery::Reader<'_>>()
                .unwrap(),
        )
        .unwrap();
    change(message.get_root().unwrap());
    capnp::serialize::write_message_to_words(&message)
}

#[test]
fn decode_applies_delivery_validation_after_reading_the_binary_schema() {
    let bad_schema = modified_wire(|mut root| root.set_schema_version(u16::MAX));
    assert!(decode_delivery(&bad_schema).is_err());
    let relative_path = modified_wire(|root| {
        root.get_request()
            .unwrap()
            .get_files()
            .unwrap()
            .get(0)
            .set_path("relative.rs");
    });
    assert!(decode_delivery(&relative_path).is_err());
    let excess_files = modified_wire(|root| {
        root.get_request().unwrap().init_files(1001);
    });
    assert!(decode_delivery(&excess_files).is_err());
    let excess_metadata = modified_wire(|root| {
        root.get_request().unwrap().init_metadata(257);
    });
    assert!(decode_delivery(&excess_metadata).is_err());
}

#[test]
fn invalid_utf8_and_duplicate_metadata_fail_closed() {
    let invalid_utf8 = modified_wire(|root| {
        root.init_producer_version(1).as_bytes_mut()[0] = 0xff;
    });
    assert!(decode_delivery(&invalid_utf8).is_err());
    let duplicate_keys = modified_wire(|root| {
        let mut entries = root.get_request().unwrap().init_metadata(2);
        for index in 0..2 {
            let mut entry = entries.reborrow().get(index);
            entry.set_key("duplicate");
            entry.set_value("value");
        }
    });
    assert!(decode_delivery(&duplicate_keys).is_err());
}

#[test]
fn binary_text_fields_share_the_existing_path_and_metadata_limits() {
    let long_path = modified_wire(|root| {
        root.get_request()
            .unwrap()
            .get_files()
            .unwrap()
            .get(0)
            .set_path("/".repeat(16 * 1024 + 1).as_str());
    });
    assert!(decode_delivery(&long_path).is_err());
    let long_key = modified_wire(|root| {
        root.get_request()
            .unwrap()
            .get_metadata()
            .unwrap()
            .get(0)
            .set_key("k".repeat(1025).as_str());
    });
    assert!(decode_delivery(&long_key).is_err());
    let long_value = modified_wire(|root| {
        root.get_request()
            .unwrap()
            .get_metadata()
            .unwrap()
            .get(0)
            .set_value("v".repeat(1024 * 1024 + 1).as_str());
    });
    assert!(decode_delivery(&long_value).is_err());
}

fn set_unknown_enum(value: capnp::dynamic_value::Builder<'_>, name: &str) {
    let mut value: capnp::dynamic_struct::Builder<'_> = value.downcast();
    let field = value.get_schema().get_field_by_name(name).unwrap();
    let capnp::introspect::TypeVariant::Enum(schema) = field.get_type().which() else {
        panic!("expected an enum field");
    };
    value
        .set(
            field,
            capnp::dynamic_value::Enum::new(u16::MAX, schema.into()).into(),
        )
        .unwrap();
}

#[test]
fn unknown_enum_values_are_rejected_instead_of_becoming_domain_defaults() {
    for field in ["checkpointKind", "pathRole"] {
        let bytes = modified_wire(|root| {
            set_unknown_enum(root.get_request().unwrap().into(), field);
        });
        assert!(decode_delivery(&bytes).is_err());
    }
    let bytes = modified_wire(|root| {
        set_unknown_enum(
            root.get_request()
                .unwrap()
                .get_stream_source()
                .unwrap()
                .into(),
            "format",
        );
    });
    assert!(decode_delivery(&bytes).is_err());
}

#[test]
fn oversized_input_is_rejected_before_encoding_or_decoding() {
    let mut delivery = fixture();
    delivery.request.files[0].content = Some("x".repeat(MAX_ENCODED_BYTES + 1));
    assert!(encode_delivery(&delivery).is_err());
    drop(delivery);
    assert!(decode_delivery(&vec![0; MAX_ENCODED_BYTES + 1]).is_err());
}

#[test]
fn aliased_file_contents_cannot_amplify_the_owned_decode_budget() {
    let mut delivery = fixture();
    let mut file = delivery.request.files[0].clone();
    file.content = None;
    delivery.request.files = vec![file; 100];
    delivery.request.files[0].content = Some("x".repeat(32 * 1024));
    let encoded = encode_delivery(&delivery).unwrap();
    let source = capnp::serialize::read_message_from_flat_slice(
        &mut encoded.as_slice(),
        capnp::message::ReaderOptions::new(),
    )
    .unwrap();
    let mut single_segment = capnp::message::Builder::new(
        capnp::message::HeapAllocator::new().first_segment_words(64 * 1024),
    );
    single_segment
        .set_root(
            source
                .get_root::<wire::checkpoint_delivery::Reader<'_>>()
                .unwrap(),
        )
        .unwrap();
    let mut bytes = capnp::serialize::write_message_to_words(&single_segment);
    assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 0);

    // Follow the standard segment/struct/list layout to rewrite content
    // pointers into aliases. The normal builder deliberately copies values.
    let root = struct_target(&bytes, 8);
    let request_pointer = root + struct_data_words(&bytes, 8) * 8 + 3 * 8;
    let request = struct_target(&bytes, request_pointer);
    let files_pointer = request + struct_data_words(&bytes, request_pointer) * 8 + 2 * 8;
    let tag = struct_target(&bytes, files_pointer);
    let data_words = struct_data_words(&bytes, tag);
    let words_per_file = data_words + ((word(&bytes, tag) >> 48) as usize);
    let first_content = tag + 8 + data_words * 8 + 8;
    let content_target = struct_target(&bytes, first_content);
    let content_layout = word(&bytes, first_content) & 0xffff_ffff_0000_0003;
    for index in 1..100 {
        let pointer = first_content + index * words_per_file * 8;
        let offset = ((content_target as isize - pointer as isize - 8) / 8) as i32;
        let value = content_layout | u64::from((offset as u32) << 2);
        bytes[pointer..pointer + 8].copy_from_slice(&value.to_le_bytes());
    }
    let error = decode_delivery(&bytes).unwrap_err().to_string();
    assert!(error.contains("Read limit exceeded"), "{error}");
}

fn word(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

fn struct_data_words(bytes: &[u8], pointer: usize) -> usize {
    ((word(bytes, pointer) >> 32) & 0xffff) as usize
}

fn struct_target(bytes: &[u8], pointer: usize) -> usize {
    let offset = (word(bytes, pointer) as u32 as i32) >> 2;
    (pointer as isize + 8 + offset as isize * 8) as usize
}
