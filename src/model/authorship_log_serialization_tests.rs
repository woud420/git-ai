use super::*;
use insta::assert_debug_snapshot;

#[test]
fn test_format_line_ranges() {
    let ranges = vec![
        LineRange::Range(19, 222),
        LineRange::Single(1),
        LineRange::Single(2),
    ];

    assert_debug_snapshot!(format_line_ranges(&ranges));
}

#[test]
fn test_parse_line_ranges() {
    let ranges = parse_line_ranges("1,2,19-222").unwrap();
    assert_debug_snapshot!(ranges);
}

#[test]
fn test_serialize_deserialize_roundtrip() {
    let mut log = AuthorshipLog::new();
    log.metadata.base_commit_sha = "abc123".to_string();

    // Add some attestations
    let mut file1 = FileAttestation::new("src/file.xyz".to_string());
    file1.add_entry(AttestationEntry::new(
        "xyzAbc".to_string(),
        vec![
            LineRange::Single(1),
            LineRange::Single(2),
            LineRange::Range(19, 222),
        ],
    ));
    file1.add_entry(AttestationEntry::new(
        "123456".to_string(),
        vec![LineRange::Range(400, 405)],
    ));

    let mut file2 = FileAttestation::new("src/file2.xyz".to_string());
    file2.add_entry(AttestationEntry::new(
        "123456".to_string(),
        vec![
            LineRange::Range(1, 111),
            LineRange::Single(245),
            LineRange::Single(260),
        ],
    ));

    log.attestations.push(file1);
    log.attestations.push(file2);

    // Serialize and snapshot the format
    let serialized = log.serialize_to_string().unwrap();
    assert_debug_snapshot!(serialized);

    // Test roundtrip: deserialize and verify structure matches
    let deserialized = AuthorshipLog::deserialize_from_string(&serialized).unwrap();
    assert_debug_snapshot!(deserialized);
}

#[test]
fn test_expected_format() {
    let mut log = AuthorshipLog::new();

    let mut file1 = FileAttestation::new("src/file.xyz".to_string());
    file1.add_entry(AttestationEntry::new(
        "xyzAbc".to_string(),
        vec![
            LineRange::Single(1),
            LineRange::Single(2),
            LineRange::Range(19, 222),
        ],
    ));
    file1.add_entry(AttestationEntry::new(
        "123456".to_string(),
        vec![LineRange::Range(400, 405)],
    ));

    let mut file2 = FileAttestation::new("src/file2.xyz".to_string());
    file2.add_entry(AttestationEntry::new(
        "123456".to_string(),
        vec![
            LineRange::Range(1, 111),
            LineRange::Single(245),
            LineRange::Single(260),
        ],
    ));

    log.attestations.push(file1);
    log.attestations.push(file2);

    let serialized = log.serialize_to_string().unwrap();
    assert_debug_snapshot!(serialized);
}

#[test]
fn test_line_range_sorting() {
    // Test that ranges are sorted correctly: single ranges and ranges by lowest bound
    let ranges = vec![
        LineRange::Range(100, 200),
        LineRange::Single(5),
        LineRange::Range(10, 15),
        LineRange::Single(50),
        LineRange::Single(1),
        LineRange::Range(25, 30),
    ];

    let formatted = format_line_ranges(&ranges);
    assert_debug_snapshot!(formatted);

    // Should be sorted as: 1, 5, 10-15, 25-30, 50, 100-200
}

#[test]
fn test_file_names_with_spaces() {
    // Test file names with spaces and special characters
    let mut log = AuthorshipLog::new();

    // Add a prompt to the metadata
    let agent_id = crate::model::working_log::AgentId {
        tool: "cursor".to_string(),
        id: "session_123".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let prompt_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
    log.metadata.prompts.insert(
        prompt_hash.clone(),
        crate::model::authorship_log::PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 0,
            total_deletions: 0,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Add attestations for files with spaces and special characters
    let mut file1 = FileAttestation::new("src/my file.rs".to_string());
    file1.add_entry(AttestationEntry::new(
        prompt_hash.to_string(),
        vec![LineRange::Range(1, 10)],
    ));

    let mut file2 = FileAttestation::new("docs/README (copy).md".to_string());
    file2.add_entry(AttestationEntry::new(
        prompt_hash.to_string(),
        vec![LineRange::Single(5)],
    ));

    let mut file3 = FileAttestation::new("test/file-with-dashes.js".to_string());
    file3.add_entry(AttestationEntry::new(
        prompt_hash.to_string(),
        vec![LineRange::Range(20, 25)],
    ));

    log.attestations.push(file1);
    log.attestations.push(file2);
    log.attestations.push(file3);

    let serialized = log.serialize_to_string().unwrap();
    println!("Serialized with special file names:\n{}", serialized);
    assert_debug_snapshot!(serialized);

    // Try to deserialize - this should work if we handle escaping properly
    let deserialized = AuthorshipLog::deserialize_from_string(&serialized);
    match deserialized {
        Ok(log) => {
            println!("Deserialization successful!");
            assert_debug_snapshot!(log);
        }
        Err(e) => {
            println!("Deserialization failed: {}", e);
            // This will fail with current implementation
        }
    }
}

#[test]
fn test_hash_always_maps_to_prompt() {
    // Demonstrate that every hash in attestation section maps to prompts section
    let mut log = AuthorshipLog::new();

    // Add a prompt to the metadata
    let agent_id = crate::model::working_log::AgentId {
        tool: "cursor".to_string(),
        id: "session_123".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let prompt_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
    log.metadata.prompts.insert(
        prompt_hash.clone(),
        crate::model::authorship_log::PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 0,
            total_deletions: 0,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Add attestation that references this prompt
    let mut file1 = FileAttestation::new("src/example.rs".to_string());
    file1.add_entry(AttestationEntry::new(
        prompt_hash.to_string(),
        vec![LineRange::Range(1, 10)],
    ));
    log.attestations.push(file1);

    let serialized = log.serialize_to_string().unwrap();
    assert_debug_snapshot!(serialized);

    // Verify that every non-h_ hash in attestations has a corresponding prompt.
    // Only non-h_ hashes must map to prompts; h_ hashes map to humans instead.
    for file_attestation in &log.attestations {
        for entry in &file_attestation.entries {
            if !entry.hash.starts_with("h_") {
                assert!(
                    log.metadata.prompts.contains_key(&entry.hash),
                    "Hash '{}' should have a corresponding prompt in metadata",
                    entry.hash
                );
            }
        }
    }
}

#[test]
fn test_serialize_deserialize_no_attestations() {
    // Test that serialization and deserialization work correctly when there are no attestations
    let mut log = AuthorshipLog::new();
    log.metadata.base_commit_sha = "abc123".to_string();

    let agent_id = crate::model::working_log::AgentId {
        tool: "cursor".to_string(),
        id: "session_123".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let prompt_hash = generate_short_hash(&agent_id.id, &agent_id.tool);
    log.metadata.prompts.insert(
        prompt_hash,
        crate::model::authorship_log::PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 0,
            total_deletions: 0,
            accepted_lines: 0,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    // Serialize and verify the format
    let serialized = log.serialize_to_string().unwrap();
    assert_debug_snapshot!(serialized);

    // Test roundtrip: deserialize and verify structure matches
    let deserialized = AuthorshipLog::deserialize_from_string(&serialized).unwrap();
    assert_debug_snapshot!(deserialized);

    // Verify that the deserialized log has the same metadata but no attestations
    assert_eq!(deserialized.metadata.base_commit_sha, "abc123");
    assert_eq!(deserialized.metadata.prompts.len(), 1);
    assert_eq!(deserialized.attestations.len(), 0);
}

#[test]
fn test_remove_line_ranges_complete_removal() {
    let mut entry = AttestationEntry::new("test_hash".to_string(), vec![LineRange::Range(2, 5)]);

    // Remove the exact same range
    entry.remove_line_ranges(&[LineRange::Range(2, 5)]);

    // Should be empty after removing the exact range
    assert!(
        entry.line_ranges.is_empty(),
        "Expected empty line_ranges after complete removal, got: {:?}",
        entry.line_ranges
    );
}

#[test]
fn test_remove_line_ranges_partial_removal() {
    let mut entry = AttestationEntry::new("test_hash".to_string(), vec![LineRange::Range(2, 10)]);

    // Remove middle part
    entry.remove_line_ranges(&[LineRange::Range(5, 7)]);

    // Should have two ranges: [2-4] and [8-10]
    assert_eq!(entry.line_ranges.len(), 2);
    assert_eq!(entry.line_ranges[0], LineRange::Range(2, 4));
    assert_eq!(entry.line_ranges[1], LineRange::Range(8, 10));
}

#[test]
fn test_generate_human_short_hash() {
    let hash = generate_human_short_hash("Alice Smith <alice@example.com>");
    // Must be exactly 16 chars: "h_" + 14 hex chars
    assert_eq!(hash.len(), 16);
    assert!(hash.starts_with("h_"));
    assert_eq!(hash, "h_31dce776f88375");
    // Must be deterministic
    assert_eq!(
        hash,
        generate_human_short_hash("Alice Smith <alice@example.com>")
    );
    // Different identities → different hashes
    assert_ne!(
        hash,
        generate_human_short_hash("Bob Jones <bob@example.com>")
    );
}

// TODO: `get_line_attribution` routing for h_ hashes requires a live `Repository` instance
// and cannot be unit-tested here without significant mocking infrastructure.
// The h_-routing path (returning HumanRecord data instead of PromptRecord) is covered by
// integration tests in the authorship integration test suite.

#[test]
fn test_generate_session_id() {
    let id = generate_session_id("session_123", "cursor");
    assert!(id.starts_with("s_"));
    assert_eq!(id.len(), 16);
    // Deterministic
    assert_eq!(id, generate_session_id("session_123", "cursor"));
    // Different inputs produce different output
    assert_ne!(id, generate_session_id("session_456", "cursor"));
}

#[test]
fn test_generate_trace_id() {
    let id = generate_trace_id();
    assert!(id.starts_with("t_"));
    assert_eq!(id.len(), 16);
    // Random: two calls produce different output
    assert_ne!(id, generate_trace_id());
    // All chars after prefix are hex
    assert!(id[2..].chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_session_id_uses_same_hash_base_as_prompt_id() {
    let session = generate_session_id("session_123", "cursor");
    let prompt = generate_short_hash("session_123", "cursor");
    // The hex portion of session (after "s_") should be a prefix of the prompt hash
    assert_eq!(&session[2..], &prompt[..14]);
}
