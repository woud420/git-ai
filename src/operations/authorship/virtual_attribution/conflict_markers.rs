/// Check whether a file's content contains git conflict markers.
///
/// Requires both an opening `<<<<<<<` and a closing `>>>>>>>` marker to avoid
/// false positives on files that happen to contain `=======` (e.g. Markdown
/// setext headings).
pub fn content_has_conflict_markers(content: &str) -> bool {
    let mut has_open = false;
    let mut has_close = false;
    for line in content.lines() {
        if line.starts_with("<<<<<<<") {
            has_open = true;
        } else if line.starts_with(">>>>>>>") {
            has_close = true;
        }
        if has_open && has_close {
            return true;
        }
    }
    false
}

/// Strip conflict markers from content, keeping the "ours" (local) side.
///
/// For `git checkout --merge` and `git switch --merge`, conflicts are written
/// with the **target branch** content first and the **local working tree** content
/// second:
///
/// ```text
/// <<<<<<< feature       ← theirs (target branch)
/// THEIRS
/// =======
/// AI_CONTENT            ← ours (local working tree / stashed VA)
/// >>>>>>> local
/// ```
///
/// We therefore keep the section **between `=======` and `>>>>>>>`** — that is
/// the local ("ours") content the stashed VA was built from.
///
/// Handles both the standard two-way conflict style and the diff3/zdiff3 style
/// which inserts a `|||||||` base section between the target and `=======`:
///
/// ```text
/// <<<<<<< feature
/// THEIRS
/// ||||||| original      ← base (diff3)
/// SHARED
/// =======
/// AI_CONTENT            ← ours (kept)
/// >>>>>>> local
/// ```
///
/// Also preserves the trailing newline of the original content so byte-level
/// attribution diffing sees the same length as the actual on-disk file.
pub fn strip_conflict_markers_keep_ours(content: &str) -> String {
    let mut result = Vec::new();
    let mut in_conflict = false;
    let mut in_ours = false; // true only while inside the ======= … >>>>>>> section

    for line in content.lines() {
        if line.starts_with("<<<<<<<") {
            in_conflict = true;
            in_ours = false; // theirs section starts — skip it
        } else if in_conflict && line.starts_with("|||||||") {
            // diff3: base section — skip
            in_ours = false;
        } else if in_conflict && line.starts_with("=======") {
            // ours (local) section starts — keep from here
            in_ours = true;
        } else if in_conflict && line.starts_with(">>>>>>>") {
            in_conflict = false;
            in_ours = false; // back to normal content
        } else if !in_conflict || in_ours {
            result.push(line);
        }
    }
    let mut out = result.join("\n");
    // Preserve the trailing newline that std::fs::read_to_string typically returns,
    // so the cleaned content has the same byte length as the actual file.
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}
