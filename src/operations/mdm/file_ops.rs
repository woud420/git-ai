pub use crate::atomic_write::write_atomic;
use crate::error::GitAiError;
use crate::model::imara_diff_utils::{LineChangeTag, compute_line_changes};
use std::fs;
use std::path::Path;

/// Read a file that may legitimately not exist yet (`None`).
pub fn read_optional(path: &Path) -> Result<Option<String>, GitAiError> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Join the per-file diffs of one install/uninstall; `None` if nothing changed.
pub fn join_diffs(diffs: impl IntoIterator<Item = String>) -> Option<String> {
    let diffs: Vec<String> = diffs.into_iter().collect();
    (!diffs.is_empty()).then(|| diffs.join("\n"))
}

/// Generate a diff between old and new content
pub fn generate_diff(path: &Path, old_content: &str, new_content: &str) -> String {
    let changes = compute_line_changes(old_content, new_content);
    let mut diff_output = String::new();
    diff_output.push_str(&format!("--- {}\n", path.display()));
    diff_output.push_str(&format!("+++ {}\n", path.display()));

    for change in changes {
        let sign = match change.tag() {
            LineChangeTag::Delete => "-",
            LineChangeTag::Insert => "+",
            LineChangeTag::Equal => " ",
        };
        diff_output.push_str(&format!("{}{}", sign, change.value()));
    }

    diff_output
}
