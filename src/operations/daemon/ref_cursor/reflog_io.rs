use super::*;

pub(crate) fn capture_reflog_start_offsets_for_worktree(worktree: &Path) -> HashMap<String, u64> {
    let mut offsets = HashMap::new();

    if let Some(git_dir) = git_dir_for_worktree(worktree) {
        let path = git_dir.join("logs").join("HEAD");
        if let Ok(metadata) = fs::metadata(&path) {
            offsets.insert(head_key(&git_dir), metadata.len());
        }
    }

    let Some(common_dir) = common_dir_for_worktree(worktree) else {
        return offsets;
    };
    let logs = common_dir.join("logs");
    let mut refs = Vec::new();
    if discover_reflog_refs(&logs, &logs, &mut refs).is_ok() {
        for reference in refs {
            if reference == "HEAD" {
                continue;
            }
            let path = logs.join(&reference);
            if let Ok(metadata) = fs::metadata(&path) {
                offsets.insert(common_key(&reference), metadata.len());
            }
        }
    }
    offsets
}

pub(crate) fn refs_at_reflog_start_offsets(
    family: &FamilyKey,
    offsets: &HashMap<String, u64>,
) -> Result<HashMap<String, String>, GitAiError> {
    let common_dir = PathBuf::from(&family.0);
    let mut refs = HashMap::new();

    for (key, offset) in offsets {
        if *offset == 0 {
            continue;
        }
        let Some((reference, path)) = reflog_reference_and_path_for_key(&common_dir, key) else {
            continue;
        };
        let Some(record) = read_reflog_record_ending_at(&path, *offset)? else {
            continue;
        };
        if valid_non_zero_oid(&record.new) {
            refs.insert(reference, record.new);
        }
    }

    Ok(refs)
}

pub(super) fn reflog_reference_and_path_for_key(
    common_dir: &Path,
    key: &str,
) -> Option<(String, PathBuf)> {
    if let Some(reference) = key.strip_prefix("common:") {
        return Some((
            reference.to_string(),
            common_dir.join("logs").join(reference),
        ));
    }
    let git_dir = key
        .strip_prefix("worktree:")
        .and_then(|value| value.strip_suffix(":HEAD"))?;
    Some((
        "HEAD".to_string(),
        PathBuf::from(git_dir).join("logs").join("HEAD"),
    ))
}

pub(super) fn read_reflog_entries(
    key: String,
    path: &Path,
    reference: &str,
    start_offset: Option<u64>,
) -> Result<Vec<CursorEntry>, GitAiError> {
    let records = read_reflog_records(path, start_offset)?;
    Ok(records
        .into_iter()
        .filter(|record| record.old != record.new)
        .map(|record| CursorEntry {
            key: key.clone(),
            path: path.to_path_buf(),
            reference: reference.to_string(),
            old: record.old,
            new: record.new,
            message: record.message,
            timestamp_secs: record.timestamp_secs,
            start_offset: record.start_offset,
            end_offset: record.end_offset,
        })
        .collect())
}

pub(super) fn read_reflog_entries_including_noops(
    key: String,
    path: &Path,
    reference: &str,
    start_offset: Option<u64>,
) -> Result<Vec<CursorEntry>, GitAiError> {
    let records = read_reflog_records(path, start_offset)?;
    Ok(records
        .into_iter()
        .map(|record| CursorEntry {
            key: key.clone(),
            path: path.to_path_buf(),
            reference: reference.to_string(),
            old: record.old,
            new: record.new,
            message: record.message,
            timestamp_secs: record.timestamp_secs,
            start_offset: record.start_offset,
            end_offset: record.end_offset,
        })
        .collect())
}

pub(super) fn read_reflog_records(
    path: &Path,
    start_offset: Option<u64>,
) -> Result<Vec<ReflogRecord>, GitAiError> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(GitAiError::IoError(error)),
    };
    let byte_len = file.metadata().map_err(GitAiError::IoError)?.len();
    let start = match start_offset {
        Some(offset) if offset > byte_len => 0,
        Some(offset) => offset,
        None => 0,
    };
    file.seek(SeekFrom::Start(start))
        .map_err(GitAiError::IoError)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(GitAiError::IoError)?;

    let mut entries = Vec::new();
    let mut offset = start;
    for raw_line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let line_start = offset;
        offset = offset.saturating_add(raw_line.len() as u64);
        if !raw_line.ends_with(b"\n") {
            continue;
        }
        let line = String::from_utf8_lossy(raw_line);
        let line = line.trim_end_matches(['\r', '\n']);
        let Some(entry) = parse_reflog_line(line, line_start, offset) else {
            continue;
        };
        if entry.end_offset > line_start {
            entries.push(entry);
        }
    }
    Ok(entries)
}

pub(super) fn read_reflog_record_ending_at(
    path: &Path,
    end_offset: u64,
) -> Result<Option<ReflogRecord>, GitAiError> {
    if end_offset == 0 {
        return Ok(None);
    }
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(GitAiError::IoError(error)),
    };
    let byte_len = file.metadata().map_err(GitAiError::IoError)?.len();
    if end_offset > byte_len {
        return Ok(None);
    }
    file.seek(SeekFrom::Start(end_offset.saturating_sub(1)))
        .map_err(GitAiError::IoError)?;
    let mut terminator = [0; 1];
    file.read_exact(&mut terminator)
        .map_err(GitAiError::IoError)?;
    if terminator[0] != b'\n' {
        return Ok(None);
    }

    let mut cursor = end_offset;
    let mut suffix = Vec::new();
    loop {
        let chunk_start = cursor.saturating_sub(8192);
        let chunk_len = (cursor - chunk_start) as usize;
        let mut chunk = vec![0; chunk_len];
        file.seek(SeekFrom::Start(chunk_start))
            .map_err(GitAiError::IoError)?;
        file.read_exact(&mut chunk).map_err(GitAiError::IoError)?;

        let search_end = if cursor == end_offset && chunk.last().is_some_and(|byte| *byte == b'\n')
        {
            chunk.len().saturating_sub(1)
        } else {
            chunk.len()
        };
        if let Some(index) = chunk[..search_end].iter().rposition(|byte| *byte == b'\n') {
            let line_start = chunk_start + index as u64 + 1;
            let mut line = chunk[index + 1..].to_vec();
            line.extend_from_slice(&suffix);
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);
            return Ok(parse_reflog_line(line, line_start, end_offset)
                .filter(|record| record.end_offset > line_start));
        }

        let mut line = chunk;
        line.extend_from_slice(&suffix);
        suffix = line;
        if chunk_start == 0 {
            let line = String::from_utf8_lossy(&suffix);
            let line = line.trim_end_matches(['\r', '\n']);
            return Ok(
                parse_reflog_line(line, 0, end_offset).filter(|record| record.end_offset > 0)
            );
        }
        cursor = chunk_start;
    }
}

pub(super) fn parse_reflog_line(
    line: &str,
    start_offset: u64,
    end_offset: u64,
) -> Option<ReflogRecord> {
    let (head, message) = line.split_once('\t').unwrap_or((line, ""));
    let mut parts = head.split_whitespace();
    let old = parts.next()?.trim();
    let new = parts.next()?.trim();
    if !is_valid_git_oid(old) || !is_valid_git_oid(new) {
        return None;
    }
    Some(ReflogRecord {
        old: old.to_string(),
        new: new.to_string(),
        message: message.to_string(),
        timestamp_secs: parse_reflog_timestamp_secs(head),
        start_offset,
        end_offset,
    })
}

pub(super) fn parse_reflog_timestamp_secs(head: &str) -> Option<i64> {
    let mut parts = head.split_whitespace().rev();
    let _timezone = parts.next()?;
    parts.next()?.parse().ok()
}

pub(super) fn discover_reflog_refs(
    root: &Path,
    current: &Path,
    out: &mut Vec<String>,
) -> Result<(), GitAiError> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            discover_reflog_refs(root, &path, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let reference = relative.to_string_lossy().replace('\\', "/");
        if reference == "ORIG_HEAD" || reference.starts_with("refs/") {
            out.push(reference);
        }
    }
    Ok(())
}
