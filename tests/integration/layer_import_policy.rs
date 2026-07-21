//! Layer import-direction policy for `src/**/*.rs`.
//!
//! Enforces the intended dependency direction from
//! `docs/decisions/2026-07-20-layered-architecture-plan.md`: the pure domain
//! (`model`, excluding the persistence adapter `model/repository`) and the
//! network adapters (`clients`) must not depend on layers above them. This is
//! GPT's "dependency enforcement" as a ~100-line policy test, not a framework.
//!
//! The test scans `use crate::…` (and a few infra crates) lines per layer and
//! fails on any forbidden import direction. Inline fully-qualified paths are not
//! scanned; a small number of known residual leaks that cannot be fixed
//! surgically within P9.2 are listed in `ALLOWED_EXCEPTIONS` with a reason.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A forbidden import for a given layer, matched against the module path in a
/// `use crate::<module>::…;` line (or an infra crate in `use <crate>::…;`).
struct Rule {
    /// Human-readable layer name for messages.
    layer: &'static str,
    /// `src`-relative path prefix the rule applies to (POSIX separators).
    applies_to: &'static str,
    /// Sub-prefixes under `applies_to` that are exempt from this rule.
    excluding: &'static [&'static str],
    /// Crate-root module segments forbidden as the first segment after `crate::`.
    forbidden_crate_modules: &'static [&'static str],
    /// External crate roots forbidden as the first path segment (e.g. `tokio`).
    forbidden_extern_crates: &'static [&'static str],
}

const RULES: &[Rule] = &[
    // Pure core domain: no orchestration/interface/network/config, no async or
    // sqlite infra.
    Rule {
        layer: "model (pure domain)",
        applies_to: "src/model",
        excluding: &["src/model/repository"],
        forbidden_crate_modules: &["operations", "cli", "clients", "config"],
        forbidden_extern_crates: &["tokio", "rusqlite"],
    },
    // Persistence adapter: may import model + config + infra, but not the
    // orchestration or interface layers.
    Rule {
        layer: "model/repository (persistence adapter)",
        applies_to: "src/model/repository",
        excluding: &[],
        forbidden_crate_modules: &["operations", "cli"],
        forbidden_extern_crates: &[],
    },
    // Network adapters: may import model + config, but not operations.
    Rule {
        layer: "clients (network adapter)",
        applies_to: "src/clients",
        excluding: &[],
        forbidden_crate_modules: &["operations"],
        forbidden_extern_crates: &[],
    },
];

/// Known residual leaks that cannot be fixed surgically without a larger
/// refactor. Each entry is one deliberate escape hatch; the list is empty
/// after P9.3 moved the last exception (`transcript::Message`).
const ALLOWED_EXCEPTIONS: &[(&str, &str)] = &[];

fn rule_applies(rule: &Rule, rel: &str) -> bool {
    if !rel.starts_with(rule.applies_to) {
        return false;
    }
    !rule.excluding.iter().any(|ex| rel.starts_with(ex))
}

/// Extract the first path segment of a `use` target, if the line is a
/// `use crate::<seg>::…` or `use <seg>::…` statement. Returns
/// `(is_crate_relative, first_segment)`.
fn parse_use_first_segment(line: &str) -> Option<(bool, &str)> {
    let line = line.trim();
    // Accept both `use …` and `pub use …` (and `pub(crate) use …`).
    let rest = line
        .strip_prefix("pub(crate) use ")
        .or_else(|| line.strip_prefix("pub use "))
        .or_else(|| line.strip_prefix("use "))?;
    const SEP: [char; 5] = [':', ';', '{', ' ', ','];
    if let Some(after_crate) = rest.strip_prefix("crate::") {
        let seg = after_crate.split(SEP).next()?;
        return Some((true, seg));
    }
    let seg = rest.split(SEP).next()?;
    Some((false, seg))
}

fn collect_src_files(root: &Path) -> Vec<(String, String)> {
    let mut files = Vec::new();
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
                files.push((rel, content));
            }
        }
    }
    files
}

#[test]
fn src_layers_respect_import_direction() {
    let root = repo_root();
    let files = collect_src_files(&root);
    let mut violations = Vec::new();

    for (rel, content) in &files {
        let excepted = ALLOWED_EXCEPTIONS.iter().any(|(f, _)| f == rel);
        for rule in RULES {
            if !rule_applies(rule, rel) {
                continue;
            }
            for (lineno, line) in content.lines().enumerate() {
                let Some((is_crate, seg)) = parse_use_first_segment(line) else {
                    continue;
                };
                let forbidden = if is_crate {
                    rule.forbidden_crate_modules.contains(&seg)
                } else {
                    rule.forbidden_extern_crates.contains(&seg)
                };
                if forbidden && !excepted {
                    violations.push(format!(
                        "{rel}:{}: {} layer must not `use {}{}::…`  ({})",
                        lineno + 1,
                        rule.layer,
                        if is_crate { "crate::" } else { "" },
                        seg,
                        line.trim(),
                    ));
                }
            }
        }
    }

    // Guard against stale exceptions: every listed file must still exist.
    for (f, _) in ALLOWED_EXCEPTIONS {
        assert!(
            files.iter().any(|(rel, _)| rel == f),
            "ALLOWED_EXCEPTIONS lists {f}, which no longer exists — remove its entry"
        );
    }

    assert!(
        violations.is_empty(),
        "layer import-direction violations:\n  {}",
        violations.join("\n  ")
    );
}
