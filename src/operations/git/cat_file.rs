//! Batched reads from Git's object database through `git cat-file --batch`.
//!
//! Keeping the byte framing and result policy here gives every caller the same
//! parser while retaining a single Git process for an arbitrary batch.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::clients::git_cli::{exec_git_dialogue, exec_git_stdin};
use crate::error::GitAiError;
use crate::operations::git::config_access_retry::with_config_access_retry;
use crate::operations::git::fast_reader::tree_entry_oid;
use crate::operations::git::repository::Repository;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Controls how malformed or missing records from Git's batch protocol are handled.
///
/// `Tolerant` is intentionally limited to notes migration, which historically
/// skipped unusable records while importing whatever valid records remained.
pub(crate) enum BatchReadPolicy {
    Strict,
    Tolerant,
    /// Strict framing, but omit missing blobs and invalid UTF-8 payloads.
    /// Blame historically treats these individual notes as unavailable.
    ValidUtf8,
}

pub(crate) fn batch_read_blob_contents(
    repo: &Repository,
    blob_oids: &[String],
) -> Result<HashMap<String, String>, GitAiError> {
    batch_read_blob_contents_with_policy(repo, blob_oids, BatchReadPolicy::Strict)
}

pub(crate) fn batch_read_blob_contents_with_policy(
    repo: &Repository,
    blob_oids: &[String],
    policy: BatchReadPolicy,
) -> Result<HashMap<String, String>, GitAiError> {
    if blob_oids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut args = repo.global_args_for_exec();
    args.push("cat-file".to_string());
    args.push("--batch".to_string());

    let stdin_data = blob_oids.join("\n") + "\n";
    let output = with_config_access_retry(
        || exec_git_stdin(&args, stdin_data.as_bytes()),
        std::thread::sleep,
    )?;
    let results = parse_batch_output_with_policy(&output.stdout, policy)?;
    if policy == BatchReadPolicy::Strict {
        for oid in blob_oids {
            if !results.contains_key(oid) {
                return Err(GitAiError::Generic(format!(
                    "missing git blob object referenced by authorship note: {}",
                    oid
                )));
            }
        }
    }
    Ok(results)
}

/// Reads each `treeish:path` name in one `git cat-file --batch`, in request order.
/// An entry is `None` when the name is missing or is not a blob.
pub(crate) fn batch_read_blobs_by_name(
    repo: &Repository,
    names: &[String],
) -> Result<Vec<Option<String>>, GitAiError> {
    if names.is_empty() {
        return Ok(Vec::new());
    }

    let mut args = repo.global_args_for_exec();
    args.push("cat-file".to_string());
    args.push("--batch".to_string());

    let stdin_data = names.join("\n") + "\n";
    let output = with_config_access_retry(
        || exec_git_stdin(&args, stdin_data.as_bytes()),
        std::thread::sleep,
    )?;
    parse_named_batch_output(&output.stdout, names.len())
}

/// Parses one record per requested name, failing unless the output holds exactly that many.
fn parse_named_batch_output(
    data: &[u8],
    expected: usize,
) -> Result<Vec<Option<String>>, GitAiError> {
    let record_count_error = |found: usize| {
        GitAiError::Generic(format!(
            "git cat-file returned {found} records for {expected} path requests"
        ))
    };
    let mut results = Vec::with_capacity(expected);
    let mut pos = 0usize;
    while results.len() < expected {
        let Some(header_len) = data[pos..].iter().position(|&b| b == b'\n') else {
            return Err(record_count_error(results.len()));
        };
        let header = std::str::from_utf8(&data[pos..pos + header_len])?;
        pos += header_len + 1;
        let Some((object_type, size)) = found_object_header(header) else {
            results.push(None);
            continue;
        };
        let content_end = pos + size;
        if content_end > data.len() {
            return Err(GitAiError::Generic(
                "Malformed cat-file --batch output: truncated content".to_string(),
            ));
        }
        results.push(
            (object_type == "blob")
                .then(|| String::from_utf8_lossy(&data[pos..content_end]).into_owned()),
        );
        pos = content_end + usize::from(data.get(content_end) == Some(&b'\n'));
    }
    if pos != data.len() {
        return Err(GitAiError::Generic(format!(
            "git cat-file returned more records than {expected} path requests"
        )));
    }
    Ok(results)
}

/// Type and size from `<oid> <type> <size>`; `None` for `<name> missing` and other notices.
fn found_object_header(header: &str) -> Option<(&str, usize)> {
    let mut parts = header.split(' ');
    let (oid, object_type, size) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || oid.is_empty() || !oid.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some((object_type, size.parse().ok()?))
}

/// A path to read in a tree-ish: `(treeish, path)`.
type PathRequest = (String, String);

/// The object spec to send for each request of a batch read on stdin.
///
/// Batch input is newline-framed (`-z` needs Git 2.38), so `<treeish>:<path>`
/// cannot carry a path that holds a newline. Those requests are rewritten by
/// [`newline_path_spec`], all in one `git cat-file --batch` dialogue: one
/// extra process whatever the number of requests and their nesting, none
/// when no path holds a newline. A request that names nothing is left out.
pub(crate) fn batch_path_specs<'a>(
    repo: &Repository,
    requests: &'a [PathRequest],
) -> Result<Vec<(&'a PathRequest, String)>, GitAiError> {
    let (with_newline, plain): (Vec<_>, Vec<_>) =
        requests.iter().partition(|(_, path)| path.contains('\n'));
    let mut specs: Vec<_> = plain
        .into_iter()
        .map(|request| (request, format!("{}:{}", request.0, request.1)))
        .collect();
    if with_newline.is_empty() {
        return Ok(specs);
    }

    let mut args = repo.global_args_for_exec();
    args.extend(["cat-file".to_string(), "--batch".to_string()]);
    exec_git_dialogue(&args, |stdin, stdout| {
        for request in with_newline {
            if let Some(spec) = newline_path_spec(stdin, stdout, request)? {
                specs.push((request, spec));
            }
        }
        Ok(())
    })?;
    Ok(specs)
}

/// A spec for `<treeish>:<path>` without the newlines `path` holds, or `None`
/// when the path names nothing.
///
/// Each path component holding a newline is looked up in the raw content of
/// its parent tree, and the spec continues from that entry's OID.
fn newline_path_spec(
    stdin: &mut dyn Write,
    stdout: &mut dyn BufRead,
    (treeish, path): &PathRequest,
) -> Result<Option<String>, GitAiError> {
    let mut object = treeish.clone();
    let mut path = path.as_str();
    while let Some(newline) = path.find('\n') {
        let start = path[..newline].rfind('/').map_or(0, |slash| slash + 1);
        let end = path[newline..]
            .find('/')
            .map_or(path.len(), |slash| newline + slash);
        let parent = match start {
            0 => format!("{object}^{{tree}}"),
            _ => format!("{object}:{}", &path[..start - 1]),
        };
        let entry = read_raw_object(stdin, stdout, &parent)?
            .filter(|parent| parent.kind == "tree")
            .and_then(|parent| {
                tree_entry_oid(&parent.content, &path[start..end], parent.oid.len() / 2)
            });
        let Some(oid) = entry else {
            return Ok(None);
        };
        path = path.get(end + 1..).unwrap_or_default();
        if path.is_empty() {
            return Ok(Some(oid));
        }
        object = oid;
    }
    Ok(Some(format!("{object}:{path}")))
}

/// An object as `git cat-file --batch` prints it.
struct RawObject {
    oid: String,
    kind: String,
    content: Vec<u8>,
}

/// Asks a `git cat-file --batch` dialogue for `spec`, which must not hold a
/// newline. `None` when Git cannot resolve it.
fn read_raw_object(
    stdin: &mut dyn Write,
    stdout: &mut dyn BufRead,
    spec: &str,
) -> Result<Option<RawObject>, GitAiError> {
    writeln!(stdin, "{spec}")?;
    stdin.flush()?;

    // `<spec> missing`, or `<oid> <type> <size>` followed by the content and
    // a newline.
    let mut header = String::new();
    stdout.read_line(&mut header)?;
    let header = header.trim_end_matches('\n');
    if header.ends_with(" missing") {
        return Ok(None);
    }
    let mut fields = header.split(' ');
    let (Some(oid), Some(kind), Some(Ok(size))) = (
        fields.next(),
        fields.next(),
        fields.next().map(str::parse::<usize>),
    ) else {
        return Err(GitAiError::Generic(format!(
            "Malformed cat-file --batch answer: {header}"
        )));
    };
    let mut content = vec![0; size + 1];
    stdout.read_exact(&mut content)?;
    content.truncate(size);
    Ok(Some(RawObject {
        oid: oid.to_string(),
        kind: kind.to_string(),
        content,
    }))
}

#[cfg(test)]
fn parse_batch_output(data: &[u8]) -> Result<HashMap<String, String>, GitAiError> {
    parse_batch_output_with_policy(data, BatchReadPolicy::Strict)
}

fn parse_batch_output_with_policy(
    data: &[u8],
    policy: BatchReadPolicy,
) -> Result<HashMap<String, String>, GitAiError> {
    let mut results = HashMap::new();
    let mut pos = 0usize;

    while pos < data.len() {
        let header_end = match data[pos..].iter().position(|&b| b == b'\n') {
            Some(idx) => pos + idx,
            None => break,
        };

        let header = match std::str::from_utf8(&data[pos..header_end]) {
            Ok(header) => header,
            Err(_error) if policy == BatchReadPolicy::Tolerant => break,
            Err(error) => return Err(error.into()),
        };

        let (oid, is_missing, size) = match policy {
            BatchReadPolicy::Strict | BatchReadPolicy::ValidUtf8 => {
                let parts: Vec<&str> = header.split_whitespace().collect();
                if parts.len() < 2 {
                    pos = header_end + 1;
                    continue;
                }

                if parts[1] == "missing" {
                    (String::new(), true, 0)
                } else if parts.len() < 3 {
                    pos = header_end + 1;
                    continue;
                } else {
                    let size: usize = parts[2].parse().map_err(|e| {
                        GitAiError::Generic(format!("Invalid size in cat-file output: {}", e))
                    })?;
                    (parts[0].to_string(), false, size)
                }
            }
            BatchReadPolicy::Tolerant => {
                let mut parts = header.trim().splitn(3, ' ');
                let Some(oid) = parts.next() else {
                    break;
                };
                let object_type = parts.next().unwrap_or("missing");
                if object_type == "missing" {
                    (String::new(), true, 0)
                } else {
                    let size = parts.next().unwrap_or("0").parse().unwrap_or(0);
                    (oid.to_string(), false, size)
                }
            }
        };

        if is_missing {
            pos = header_end + 1;
            continue;
        }

        let content_start = header_end + 1;
        let content_end = content_start + size;
        if content_end > data.len() {
            if policy == BatchReadPolicy::Tolerant {
                break;
            }
            return Err(GitAiError::Generic(
                "Malformed cat-file --batch output: truncated content".to_string(),
            ));
        }

        let content = match policy {
            BatchReadPolicy::Strict => {
                String::from_utf8_lossy(&data[content_start..content_end]).to_string()
            }
            BatchReadPolicy::Tolerant | BatchReadPolicy::ValidUtf8 => {
                let Ok(content) = std::str::from_utf8(&data[content_start..content_end]) else {
                    pos = content_end;
                    if pos < data.len() && data[pos] == b'\n' {
                        pos += 1;
                    }
                    continue;
                };
                content.to_string()
            }
        };
        results.insert(oid, content);

        pos = content_end;
        if pos < data.len() && data[pos] == b'\n' {
            pos += 1;
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
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

        let result =
            batch_read_blob_contents(repo.gitai_repo(), &[oid.clone(), oid.clone()]).unwrap();

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

    #[test]
    fn named_batch_keeps_request_order_and_framing() {
        let data = b"HEAD:gone with spaces missing\nabc tree 3\nxyz\ndef blob 2\n\xffx\n";
        assert_eq!(
            parse_named_batch_output(data, 3).unwrap(),
            vec![None, None, Some("\u{fffd}x".to_string())]
        );
        for (data, expected) in [
            (b"abc blob 2\nhi\n".as_slice(), 2),
            (b"abc blob 2\nhi\nx missing\n", 1),
            (b"abc blob 9\nhi\n", 1),
        ] {
            assert!(parse_named_batch_output(data, expected).is_err());
        }
    }

    #[test]
    fn reads_paths_at_treeishes_skipping_missing_paths_and_trees() {
        let repo = TmpRepo::new().unwrap();
        repo.write_file("a.txt", "one\n", false).unwrap();
        repo.write_file("dir/b c.txt", "spaced\n", false).unwrap();
        let first = repo.commit_all("first").unwrap();
        repo.write_file("a.txt", "two\n", false).unwrap();
        let second = repo.commit_all("second").unwrap();
        let request = |treeish: &str, path: &str| (treeish.to_string(), path.to_string());
        let requests = [
            request(&first, "a.txt"),
            request(&second, "a.txt"),
            request(&second, "dir"),
            request(&second, "dir/b c.txt"),
            request(&first, "dir/b c.txt"),
            request(&second, "gone missing"),
            request(&second, "a.txt"),
        ];

        let contents = crate::operations::git::repository::batch_read_paths_at_treeishes(
            repo.gitai_repo(),
            &requests,
        )
        .unwrap();

        let expected = HashMap::from([
            (request(&first, "a.txt"), "one\n".to_string()),
            (request(&second, "a.txt"), "two\n".to_string()),
            (request(&second, "dir/b c.txt"), "spaced\n".to_string()),
            (request(&first, "dir/b c.txt"), "spaced\n".to_string()),
        ]);
        assert_eq!(contents, expected);
    }
}
