use super::*;

#[test]
fn test_parse_batch_check_blob_oid_accepts_sha1_and_sha256() {
    let sha1 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa blob 10";
    let sha256 = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb blob 20";
    let invalid = "cccccccc blob 10";

    assert_eq!(
        parse_batch_check_blob_oid(sha1),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string())
    );
    assert_eq!(
        parse_batch_check_blob_oid(sha256),
        Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string())
    );
    assert_eq!(parse_batch_check_blob_oid(invalid), None);
}

#[test]
fn test_notes_path_for_object() {
    // Short SHA (edge case)
    assert_eq!(notes_path_for_object("a"), "a");
    assert_eq!(notes_path_for_object("ab"), "ab");

    // Normal SHA (40 chars)
    assert_eq!(
        notes_path_for_object("abcdef1234567890abcdef1234567890abcdef12"),
        "ab/cdef1234567890abcdef1234567890abcdef12"
    );

    // SHA-256 (64 chars)
    assert_eq!(
        notes_path_for_object("abc1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcd"),
        "ab/c1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcd"
    );
}

#[test]
fn test_flat_note_pathspec_for_commit() {
    let sha = "abcdef1234567890abcdef1234567890abcdef12";
    let pathspec = flat_note_pathspec_for_commit(sha);
    assert_eq!(
        pathspec,
        "refs/notes/ai:abcdef1234567890abcdef1234567890abcdef12"
    );
}

#[test]
fn test_fanout_note_pathspec_for_commit() {
    let sha = "abcdef1234567890abcdef1234567890abcdef12";
    let pathspec = fanout_note_pathspec_for_commit(sha);
    assert_eq!(
        pathspec,
        "refs/notes/ai:ab/cdef1234567890abcdef1234567890abcdef12"
    );
}

#[test]
fn test_sanitize_remote_name() {
    assert_eq!(sanitize_remote_name("origin"), "origin");
    assert_eq!(sanitize_remote_name("my-remote"), "my-remote");
    assert_eq!(sanitize_remote_name("remote_123"), "remote_123");
    assert_eq!(
        sanitize_remote_name("remote/with/slashes"),
        "remote_with_slashes"
    );
    assert_eq!(
        sanitize_remote_name("remote@with#special$chars"),
        "remote_with_special_chars"
    );
    assert_eq!(sanitize_remote_name("has spaces"), "has_spaces");
}

#[test]
fn test_tracking_ref_for_remote() {
    assert_eq!(
        tracking_ref_for_remote("origin"),
        "refs/notes/ai-remote/origin"
    );
    assert_eq!(
        tracking_ref_for_remote("upstream"),
        "refs/notes/ai-remote/upstream"
    );
    assert_eq!(
        tracking_ref_for_remote("my-fork"),
        "refs/notes/ai-remote/my-fork"
    );
    // Special characters get sanitized
    assert_eq!(
        tracking_ref_for_remote("remote/with/slashes"),
        "refs/notes/ai-remote/remote_with_slashes"
    );
}
