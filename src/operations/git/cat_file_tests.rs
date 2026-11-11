use super::*;
use crate::operations::git::test_utils::TmpRepo;

/// A tree entry can be named with a newline, which the newline-framed
/// batch input cannot carry. Such a path is read like any other, at any
/// depth, and does not disturb the other requests of its batch.
#[cfg(not(windows))]
#[test]
fn paths_holding_newlines_are_read_with_the_rest_of_the_batch() {
    let tmp = TmpRepo::new().unwrap();
    let present = [
        ("plain.txt", "plain\n"),
        ("new\nline.txt", "leaf\n"),
        ("dir/new\nline.txt", "nested\n"),
        ("new\ndir/leaf.txt", "under\n"),
        ("new\ndir/new\nleaf.txt", "twice\n"),
    ];
    for (path, content) in present {
        tmp.write_file(path, content, false).unwrap();
    }
    let commit = tmp.commit_all("base").unwrap();
    let absent = [
        "absent.txt",
        "absent blob",
        "dir/absent\nfile.txt",
        "absent\ndir/leaf.txt",
        "plain.txt/under\nfile",
        "new\ndir",
    ];
    let request = |path: &str| (commit.clone(), path.to_string());
    let requests: Vec<(String, String)> = present
        .iter()
        .map(|(path, _)| *path)
        .chain(absent)
        .map(request)
        .collect();

    let read = crate::operations::git::repository::batch_read_paths_at_treeishes(
        tmp.gitai_repo(),
        &requests,
    )
    .unwrap();

    let expected: HashMap<(String, String), String> = present
        .iter()
        .map(|(path, content)| (request(path), content.to_string()))
        .collect();
    assert_eq!(read, expected);
}

#[test]
fn raw_object_dialogue_reads_binary_content_and_missing_specs() {
    let mut requests = Vec::new();
    let mut answers: &[u8] = b"a:x y missing\nabc tree 3\n\x00\n\xff\ndef blob 0\n\n";

    let missing = read_raw_object(&mut requests, &mut answers, "a:x y").unwrap();
    assert!(missing.is_none());
    let tree = read_raw_object(&mut requests, &mut answers, "abc").unwrap();
    let tree = tree.unwrap();
    assert_eq!((tree.oid.as_str(), tree.kind.as_str()), ("abc", "tree"));
    assert_eq!(tree.content, b"\x00\n\xff");
    let empty = read_raw_object(&mut requests, &mut answers, "def").unwrap();
    assert!(empty.unwrap().content.is_empty());
    assert_eq!(requests, b"a:x y\nabc\ndef\n");

    for mut malformed in [b"abc tree 9\nshort\n".as_slice(), b"abc tree\n", b""] {
        assert!(read_raw_object(&mut requests, &mut malformed, "abc").is_err());
    }
}

#[test]
fn parses_empty_output() {
    assert!(parse_batch_output(b"").unwrap().is_empty());
}

#[test]
fn skips_missing_objects() {
    assert!(parse_batch_output(b"abc123 missing\n").unwrap().is_empty());
}

#[test]
fn tolerant_policy_skips_missing_objects() {
    let data = b"abc123 missing\ndef456 blob 5\nworld\n";

    let result = parse_batch_output_with_policy(data, BatchReadPolicy::Tolerant).unwrap();

    assert_eq!(result.get("def456"), Some(&"world".to_string()));
    assert!(!result.contains_key("abc123"));
}

#[test]
fn utf8_policy_keeps_valid_neighbors_and_rejects_broken_framing() {
    let data = b"missing missing\nbad blob 2\n\xffx\ngood blob 5\nhello\n";
    let result = parse_batch_output_with_policy(data, BatchReadPolicy::ValidUtf8).unwrap();
    assert_eq!(
        result,
        HashMap::from([("good".to_owned(), "hello".to_owned())])
    );
    for malformed in [b"bad blob invalid\n".as_slice(), b"bad blob 20\npartial\n"] {
        assert!(parse_batch_output_with_policy(malformed, BatchReadPolicy::ValidUtf8).is_err());
    }
}

#[test]
fn parses_single_blob() {
    let result = parse_batch_output(b"abc123 blob 11\nhello world\n").unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result.get("abc123"), Some(&"hello world".to_string()));
}

#[test]
fn parses_multiple_blobs() {
    let data = b"abc123 blob 5\nhello\ndef456 blob 5\nworld\n";
    let result = parse_batch_output(data).unwrap();

    assert_eq!(result.len(), 2);
    assert_eq!(result.get("abc123"), Some(&"hello".to_string()));
    assert_eq!(result.get("def456"), Some(&"world".to_string()));
}

#[test]
fn preserves_embedded_newlines() {
    let result = parse_batch_output(b"abc123 blob 12\nhello\nworld\n\n").unwrap();

    assert_eq!(result.get("abc123"), Some(&"hello\nworld\n".to_string()));
}

#[test]
fn duplicate_oid_keeps_last_record() {
    let data = b"abc123 blob 3\none\nabc123 blob 3\ntwo\n";
    let result = parse_batch_output(data).unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result.get("abc123"), Some(&"two".to_string()));
}

#[test]
fn decodes_blob_content_lossily() {
    let result = parse_batch_output(b"abc123 blob 2\n\xffx\n").unwrap();

    assert_eq!(result.get("abc123"), Some(&"\u{fffd}x".to_string()));
}

#[test]
fn preserves_truncated_content_error() {
    let error = parse_batch_output(b"abc123 blob 20\nhello").unwrap_err();

    assert_eq!(
        error.to_string(),
        "Generic error: Malformed cat-file --batch output: truncated content"
    );
}

#[test]
fn rejects_invalid_size() {
    let error = parse_batch_output(b"abc123 blob notanumber\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "Generic error: Invalid size in cat-file output: invalid digit found in string"
    );
}

#[test]
fn tolerant_policy_keeps_legacy_invalid_size_behavior() {
    let result =
        parse_batch_output_with_policy(b"abc123 blob notanumber\n", BatchReadPolicy::Tolerant)
            .unwrap();

    assert_eq!(result.get("abc123"), Some(&String::new()));
}

#[test]
fn tolerant_policy_returns_partial_results_for_truncated_content() {
    let data = b"abc123 blob 5\nhello\ndef456 blob 20\npartial";

    let result = parse_batch_output_with_policy(data, BatchReadPolicy::Tolerant).unwrap();

    assert_eq!(result.get("abc123"), Some(&"hello".to_string()));
    assert!(!result.contains_key("def456"));
}

#[test]
fn tolerant_policy_skips_invalid_utf8_content() {
    let result = parse_batch_output_with_policy(
        b"abc123 blob 2\n\xffx\ndef456 blob 5\nworld\n",
        BatchReadPolicy::Tolerant,
    )
    .unwrap();

    assert!(!result.contains_key("abc123"));
    assert_eq!(result.get("def456"), Some(&"world".to_string()));
}

#[test]
fn preserves_unvalidated_object_type_and_oid() {
    let result = parse_batch_output(b"not-an-oid tree 3\none\n").unwrap();

    assert_eq!(result.get("not-an-oid"), Some(&"one".to_string()));
}

#[test]
fn skips_malformed_header() {
    assert!(parse_batch_output(b"abc123\n").unwrap().is_empty());
}

#[test]
fn skips_header_without_object_size() {
    assert!(parse_batch_output(b"abc123 blob\n").unwrap().is_empty());
}

#[test]
fn ignores_trailing_header_without_newline() {
    assert!(parse_batch_output(b"abc123 blob 3").unwrap().is_empty());
}

#[test]
fn rejects_non_utf8_header() {
    let error = parse_batch_output(b"\xff blob 1\nx\n").unwrap_err();

    assert!(matches!(error, GitAiError::Utf8Error(_)));
}

#[test]
fn empty_read_does_not_require_objects() {
    let repo = TmpRepo::new().unwrap();

    assert!(
        batch_read_blob_contents(repo.gitai_repo(), &[])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reads_duplicate_real_git_objects() {
    let repo = TmpRepo::new().unwrap();
    repo.write_file("blob.txt", "hello\nworld\n", false)
        .unwrap();
    let oid = repo
        .git_command(&["hash-object", "-w", "blob.txt"])
        .unwrap()
        .trim()
        .to_string();

    let result = batch_read_blob_contents(repo.gitai_repo(), &[oid.clone(), oid.clone()]).unwrap();

    assert_eq!(result.len(), 1);
    assert_eq!(result.get(&oid), Some(&"hello\nworld\n".to_string()));
}

#[test]
fn preserves_missing_object_error() {
    let repo = TmpRepo::new().unwrap();
    let missing_oid = "0000000000000000000000000000000000000000".to_string();

    let error = batch_read_blob_contents(repo.gitai_repo(), std::slice::from_ref(&missing_oid))
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        format!(
            "Generic error: missing git blob object referenced by authorship note: {missing_oid}"
        )
    );
}

#[test]
fn tolerant_read_allows_missing_objects() {
    let repo = TmpRepo::new().unwrap();
    repo.write_file("blob.txt", "hello\n", false).unwrap();
    let oid = repo
        .git_command(&["hash-object", "-w", "blob.txt"])
        .unwrap()
        .trim()
        .to_string();
    let missing_oid = "0000000000000000000000000000000000000000".to_string();

    let result = batch_read_blob_contents_with_policy(
        repo.gitai_repo(),
        &[oid.clone(), missing_oid.clone()],
        BatchReadPolicy::Tolerant,
    )
    .unwrap();

    assert_eq!(result.get(&oid), Some(&"hello\n".to_string()));
    assert!(!result.contains_key(&missing_oid));
}
