use super::{
    AgentId, AttestationEntry, AuthorshipLog, FileAttestation, HashMap, LineRange, PromptRecord,
    generate_short_hash,
};
use git_ai::operations::authorship::stats::accepted_lines_from_attestations;
use git_ai::operations::authorship::stats::line_range_overlap_len;

#[test]
fn test_accepted_lines_no_authorship_log() {
    let added_lines: HashMap<String, Vec<u32>> = HashMap::new();
    let (accepted, known_human, per_tool) =
        accepted_lines_from_attestations(None, &added_lines, false);
    assert_eq!(accepted, 0);
    assert_eq!(known_human, 0);
    assert!(per_tool.is_empty());
}

#[test]
fn test_accepted_lines_merge_commit() {
    // Even with a real authorship log, merge commits should short-circuit to (0, empty)
    let mut log = AuthorshipLog::new();
    let agent_id = AgentId {
        tool: "cursor".to_string(),
        id: "session_1".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let hash = generate_short_hash(&agent_id.id, &agent_id.tool);
    log.metadata.prompts.insert(
        hash.clone(),
        PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 5,
            total_deletions: 0,
            accepted_lines: 5,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    let mut file_att = FileAttestation::new("foo.rs".to_string());
    file_att.add_entry(AttestationEntry::new(hash, vec![LineRange::Range(1, 3)]));
    log.attestations.push(file_att);

    let mut added_lines: HashMap<String, Vec<u32>> = HashMap::new();
    added_lines.insert("foo.rs".to_string(), vec![1, 2, 3]);

    let (accepted, known_human, per_tool) =
        accepted_lines_from_attestations(Some(&log), &added_lines, true);
    assert_eq!(accepted, 0);
    assert_eq!(known_human, 0);
    assert!(per_tool.is_empty());
}

#[test]
fn test_accepted_lines_no_matching_files() {
    let mut log = AuthorshipLog::new();
    let agent_id = AgentId {
        tool: "cursor".to_string(),
        id: "session_2".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let hash = generate_short_hash(&agent_id.id, &agent_id.tool);
    log.metadata.prompts.insert(
        hash.clone(),
        PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 3,
            total_deletions: 0,
            accepted_lines: 3,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    let mut file_att = FileAttestation::new("foo.rs".to_string());
    file_att.add_entry(AttestationEntry::new(hash, vec![LineRange::Range(1, 3)]));
    log.attestations.push(file_att);

    // added_lines has "bar.rs" but NOT "foo.rs"
    let mut added_lines: HashMap<String, Vec<u32>> = HashMap::new();
    added_lines.insert("bar.rs".to_string(), vec![1, 2, 3]);

    let (accepted, known_human, per_tool) =
        accepted_lines_from_attestations(Some(&log), &added_lines, false);
    assert_eq!(accepted, 0);
    assert_eq!(known_human, 0);
    assert!(per_tool.is_empty());
}

#[test]
fn test_accepted_lines_basic_match() {
    let mut log = AuthorshipLog::new();
    let agent_id = AgentId {
        tool: "cursor".to_string(),
        id: "session_3".to_string(),
        model: "claude-3-sonnet".to_string(),
    };
    let hash = generate_short_hash(&agent_id.id, &agent_id.tool);
    log.metadata.prompts.insert(
        hash.clone(),
        PromptRecord {
            agent_id,
            human_author: None,
            total_additions: 3,
            total_deletions: 0,
            accepted_lines: 3,
            overriden_lines: 0,
            custom_attributes: None,
            messages_url: None,
        },
    );

    let mut file_att = FileAttestation::new("foo.rs".to_string());
    file_att.add_entry(AttestationEntry::new(
        hash.clone(),
        vec![LineRange::Range(1, 3)],
    ));
    log.attestations.push(file_att);

    let mut added_lines: HashMap<String, Vec<u32>> = HashMap::new();
    added_lines.insert("foo.rs".to_string(), vec![1, 2, 3]);

    let (accepted, known_human, per_tool) =
        accepted_lines_from_attestations(Some(&log), &added_lines, false);
    assert_eq!(accepted, 3);
    assert_eq!(known_human, 0);

    // Verify per-tool breakdown contains the right key
    let expected_key = "cursor::claude-3-sonnet".to_string();
    assert_eq!(per_tool.get(&expected_key), Some(&3));
}

// --- line_range_overlap_len tests ---

#[test]
fn test_overlap_single_hit() {
    let count = line_range_overlap_len(&LineRange::Single(5), &[3, 5, 7]);
    assert_eq!(count, 1);
}

#[test]
fn test_overlap_single_miss() {
    let count = line_range_overlap_len(&LineRange::Single(4), &[3, 5, 7]);
    assert_eq!(count, 0);
}

#[test]
fn test_overlap_range_full() {
    let count = line_range_overlap_len(&LineRange::Range(3, 7), &[3, 4, 5, 6, 7]);
    assert_eq!(count, 5);
}

#[test]
fn test_overlap_range_partial() {
    // Range [4, 8] intersected with [3, 5, 7, 9]: only 5 and 7 are in range
    let count = line_range_overlap_len(&LineRange::Range(4, 8), &[3, 5, 7, 9]);
    assert_eq!(count, 2);
}

#[test]
fn test_overlap_range_miss() {
    let count = line_range_overlap_len(&LineRange::Range(10, 20), &[1, 2, 3]);
    assert_eq!(count, 0);
}

#[test]
fn test_overlap_range_empty_added() {
    let count = line_range_overlap_len(&LineRange::Range(1, 10), &[]);
    assert_eq!(count, 0);
}

#[test]
fn test_line_range_overlap_edge_cases() {
    // Empty added_lines
    assert_eq!(line_range_overlap_len(&LineRange::Single(5), &[]), 0);
    assert_eq!(line_range_overlap_len(&LineRange::Range(1, 10), &[]), 0);

    // Range with start == end
    assert_eq!(line_range_overlap_len(&LineRange::Range(5, 5), &[5]), 1);
    assert_eq!(line_range_overlap_len(&LineRange::Range(5, 5), &[4, 6]), 0);

    // Range before all lines
    assert_eq!(
        line_range_overlap_len(&LineRange::Range(1, 2), &[10, 20, 30]),
        0
    );

    // Range after all lines
    assert_eq!(
        line_range_overlap_len(&LineRange::Range(50, 60), &[10, 20, 30]),
        0
    );

    // Range partially overlapping
    assert_eq!(
        line_range_overlap_len(&LineRange::Range(5, 15), &[1, 3, 10, 12, 20]),
        2
    );
}
