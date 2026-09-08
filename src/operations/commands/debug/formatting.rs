use std::fmt::Write as _;

pub(super) fn debug_progress(message: impl AsRef<str>) {
    eprintln!(
        "[{}] git-ai debug: {}",
        chrono::Utc::now().to_rfc3339(),
        message.as_ref()
    );
}

pub(super) fn append_indented_block(out: &mut String, content: &str) {
    append_indented_block_with_prefix(out, content, "  ");
}

pub(super) fn append_indented_block_with_prefix(out: &mut String, content: &str, prefix: &str) {
    if content.trim().is_empty() {
        let _ = writeln!(out, "{}<empty>", prefix);
        return;
    }
    for line in content.lines() {
        let _ = writeln!(out, "{}{}", prefix, line);
    }
}
