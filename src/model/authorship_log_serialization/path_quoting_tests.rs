use super::*;

fn log_for_path(path: &str) -> AuthorshipLog {
    let mut log = AuthorshipLog::new();
    let mut file = FileAttestation::new(path.to_string());
    file.add_entry(AttestationEntry::new(
        "abc123".to_string(),
        vec![LineRange::Range(1, 2)],
    ));
    log.attestations.push(file);
    log
}

fn round_trip_path(path: &str) -> String {
    let serialized = log_for_path(path).serialize_to_string().unwrap();
    let parsed = AuthorshipLog::deserialize_from_string(&serialized)
        .unwrap_or_else(|e| panic!("note for path {path:?} was rejected: {e}"));
    assert_eq!(parsed.attestations.len(), 1, "path {path:?}");
    parsed.attestations[0].file_path.clone()
}

/// The path line exactly as it was written before quote/divider paths were
/// handled: quoted only when it contains a space, tab or newline.
fn legacy_path_line(path: &str) -> String {
    if path.contains([' ', '\t', '\n']) {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

#[test]
fn paths_that_look_quoted_or_like_the_divider_round_trip() {
    for path in ["\"", "\"\"", "\"a\"", "\"dir/a.rs\"", "---"] {
        assert_eq!(round_trip_path(path), path);
    }
}

#[test]
fn path_line_bytes_are_unchanged_for_paths_that_already_round_tripped() {
    let paths = [
        "src/main.rs",
        "src/my file.rs",
        "tab\there.txt",
        " leading-space",
        "\"a b\"",
        "\"leading-quote-only",
        "trailing-quote-only\"",
        "mid\"dle",
        "'single'",
        "dir/---",
        "----",
        "--",
        "--- ",
        "ünïcode/文件.rs",
    ];
    for path in paths {
        let serialized = log_for_path(path).serialize_to_string().unwrap();
        let path_line = serialized.lines().next().unwrap();
        assert_eq!(path_line, legacy_path_line(path), "path {path:?}");
        assert_eq!(round_trip_path(path), path);
    }
}

#[test]
fn bare_quote_path_line_from_an_older_note_does_not_panic() {
    let note = format!(
        "\"\n  abc123 1-2\n---\n{}",
        serde_json::to_string_pretty(&AuthorshipMetadata::new()).unwrap()
    );
    let parsed = AuthorshipLog::deserialize_from_string(&note).unwrap();
    assert_eq!(parsed.attestations[0].file_path, "\"");
}
