//! Batched reads from Git's object database through `git cat-file --batch`.
//!
//! Keeping the byte framing and result policy here gives every caller the same
//! parser while retaining a single Git process for an arbitrary batch.

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::clients::git_cli::{exec_git_dialogue, exec_git_stdin};
use crate::error::GitAiError;
use crate::operations::git::config_access_retry::with_config_access_retry;
use crate::operations::git::fast_reader::{DIRECTORY_MODE, tree_entry};
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
                tree_entry(&parent.content, &path[start..end], parent.oid.len() / 2)
            });
        let Some((mode, oid)) = entry else {
            return Ok(None);
        };
        path = path.get(end + 1..).unwrap_or_default();
        if path.is_empty() {
            return Ok(Some(oid));
        }
        // Only a directory has entries below it. A gitlink names a commit,
        // and that commit's tree is not part of this one.
        if mode != DIRECTORY_MODE {
            return Ok(None);
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
#[path = "cat_file_tests.rs"]
mod tests;
