use std::fs;
use std::path::{Path, PathBuf};

const REVIEW_PROCESS_FILES: &[&str] = &[
    "AGENTS.md",
    "scripts/ai",
    "docs/session-event-attribution-recovery-plan.md",
    "tests/integration/checkpoint_unit.rs",
];
const DEVIN_REVIEW_BOT_NAME: &str = "devin";

// Regression coverage for ENG-351.
#[test]
fn eng_351_review_workflow_has_no_unused_bot_assumptions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = REVIEW_PROCESS_FILES
        .iter()
        .map(|path| root.join(path))
        .collect::<Vec<_>>();
    collect_files(&root.join(".github"), &mut files);
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for (line_index, line) in contents.lines().enumerate() {
            if line.to_ascii_lowercase().contains(DEVIN_REVIEW_BOT_NAME) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                violations.push(format!("{}:{}", relative.display(), line_index + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "fork review workflow still assumes an unused review bot:\n{}",
        violations.join("\n")
    );
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let file_type = entry.file_type().expect("failed to read entry file type");
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(&path, files);
        } else if file_type.is_file() {
            files.push(path);
        }
    }
}
