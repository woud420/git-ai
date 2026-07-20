//! 600-line cap for `src/**/*.rs`, enforced as a shrinking ratchet.
//!
//! `.file-length-baseline.txt` lists the files that predate the cap together
//! with their recorded ceiling. This test fails when:
//! - a src file NOT in the baseline exceeds the cap (new oversized code),
//! - a baselined file grows past its recorded ceiling (offenders may only
//!   shrink), or
//! - a baselined file is at or under the cap, or no longer exists (its entry
//!   must be deleted so the baseline only ever shrinks).
//!
//! A baselined file that shrinks but stays above the cap passes without a
//! baseline update; lowering its ceiling voluntarily is welcome.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAX_LINES: usize = 600;
const BASELINE_FILE: &str = ".file-length-baseline.txt";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_baseline(root: &Path) -> BTreeMap<String, usize> {
    let content = std::fs::read_to_string(root.join(BASELINE_FILE))
        .unwrap_or_else(|e| panic!("failed to read {BASELINE_FILE}: {e}"));
    let mut baseline = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (path, ceiling) = line.split_once('\t').unwrap_or_else(|| {
            panic!("malformed baseline line (expected 'path<TAB>lines'): {line}")
        });
        let ceiling: usize = ceiling
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("malformed ceiling in baseline line: {line}"));
        baseline.insert(path.to_string(), ceiling);
    }
    baseline
}

fn collect_src_line_counts(root: &Path) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    let mut stack = vec![root.join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src dir") {
            let entry = entry.expect("read dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let content = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
                let rel = path
                    .strip_prefix(root)
                    .expect("src path under repo root")
                    .to_string_lossy()
                    .replace('\\', "/");
                counts.insert(rel, content.lines().count());
            }
        }
    }
    counts
}

#[test]
fn src_files_stay_under_the_line_cap_or_shrink() {
    let root = repo_root();
    let baseline = read_baseline(&root);
    let counts = collect_src_line_counts(&root);
    let mut violations = Vec::new();

    for (path, lines) in &counts {
        match baseline.get(path) {
            None => {
                if *lines > MAX_LINES {
                    violations.push(format!(
                        "{path}: {lines} lines exceeds the {MAX_LINES}-line cap; split it \
                         (do not add new entries to {BASELINE_FILE})"
                    ));
                }
            }
            Some(ceiling) => {
                if *lines > *ceiling {
                    violations.push(format!(
                        "{path}: grew from its recorded ceiling of {ceiling} to {lines} lines; \
                         baselined offenders may only shrink"
                    ));
                } else if *lines <= MAX_LINES {
                    violations.push(format!(
                        "{path}: now {lines} lines (at or under the cap) — remove its entry \
                         from {BASELINE_FILE} so the ratchet keeps tightening"
                    ));
                }
            }
        }
    }

    for path in baseline.keys() {
        if !counts.contains_key(path) {
            violations.push(format!(
                "{path}: listed in {BASELINE_FILE} but no longer exists — remove its entry"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "file-length ratchet violations:\n  {}",
        violations.join("\n  ")
    );
}
