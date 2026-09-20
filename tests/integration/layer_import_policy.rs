//! Layer dependency guard for `src/**/*.rs`.
//!
//! Enforces the audited boundaries in `docs/architecture/inventory.md`.
//! This lexical check covers flat/multiline imports and rooted paths; nested
//! import groups fail closed. It is not a Rust resolver: macros, re-exports,
//! trait implementations and file-relative paths in inline modules need review.

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
    applies_to: &'static [&'static str],
    /// Empty outside the audited pure core; otherwise every crate path must match.
    allowed_crate_prefixes: &'static [&'static str],
    /// Sub-prefixes under `applies_to` that are exempt from this rule.
    excluding: &'static [&'static str],
    /// Crate-root module segments forbidden as the first segment after `crate::`.
    forbidden_crate_modules: &'static [&'static str],
    /// External crate roots forbidden as the first path segment (e.g. `tokio`).
    forbidden_extern_crates: &'static [&'static str],
    /// Module prefixes denied in imports and, for the pure core/model, inline paths.
    forbidden_prefixes: &'static [&'static str],
}

const PURE_CORE_FORBIDDEN_PREFIXES: &[&str] = &[
    "std::fs",
    "std::io",
    "std::env",
    "std::process",
    "std::net",
    "std::os",
    "crate::model::repository",
    "crate::model::clock",
];
const PURE_CORE_FORBIDDEN_CRATES: &[&str] = &[
    "tokio",
    "rusqlite",
    "ureq",
    "interprocess",
    "serde_json",
    "rand",
    "tracing",
    "chrono",
];
const PURE_GIT_KERNELS: &[&str] = &[
    "src/operations/git/cli_parser.rs",
    "src/operations/git/command_classification.rs",
    "src/operations/git/command_policy.rs",
];

const RULES: &[Rule] = &[
    Rule {
        layer: "model (adapter-independent)",
        applies_to: &["src/model/"],
        allowed_crate_prefixes: &[],
        excluding: &["src/model/repository/"],
        forbidden_crate_modules: &["operations", "cli", "clients", "config", "metrics"],
        forbidden_extern_crates: &["tokio", "rusqlite"],
        forbidden_prefixes: &["crate::model::repository"],
    },
    Rule {
        layer: "model/repository (persistence adapter)",
        applies_to: &["src/model/repository/"],
        allowed_crate_prefixes: &[],
        excluding: &[],
        forbidden_crate_modules: &["operations", "cli"],
        forbidden_extern_crates: &[],
        forbidden_prefixes: &[],
    },
    Rule {
        layer: "clients (network adapter)",
        applies_to: &["src/clients/"],
        allowed_crate_prefixes: &[],
        excluding: &[],
        forbidden_crate_modules: &["operations"],
        forbidden_extern_crates: &[],
        forbidden_prefixes: &[],
    },
    Rule {
        layer: "daemon analyzers (pure core)",
        applies_to: &["src/operations/daemon/analyzers/"],
        allowed_crate_prefixes: &[
            "crate::error",
            "crate::model",
            "crate::operations::daemon::analyzers",
            "crate::operations::git::cli_parser",
            "crate::operations::git::command_classification",
            "crate::operations::git::command_policy",
        ],
        excluding: &[],
        forbidden_crate_modules: &[],
        forbidden_extern_crates: PURE_CORE_FORBIDDEN_CRATES,
        forbidden_prefixes: PURE_CORE_FORBIDDEN_PREFIXES,
    },
    Rule {
        layer: "daemon reducer (pure core)",
        applies_to: &["src/operations/daemon/reducer.rs"],
        allowed_crate_prefixes: &[
            "crate::error",
            "crate::model",
            "crate::operations::daemon::analyzers",
        ],
        excluding: &[],
        forbidden_crate_modules: &[],
        forbidden_extern_crates: PURE_CORE_FORBIDDEN_CRATES,
        forbidden_prefixes: PURE_CORE_FORBIDDEN_PREFIXES,
    },
    Rule {
        layer: "Git grammar and command policy (pure core)",
        applies_to: PURE_GIT_KERNELS,
        allowed_crate_prefixes: &["crate::operations::git::command_policy"],
        excluding: &[],
        forbidden_crate_modules: &[],
        forbidden_extern_crates: PURE_CORE_FORBIDDEN_CRATES,
        forbidden_prefixes: PURE_CORE_FORBIDDEN_PREFIXES,
    },
    Rule {
        layer: "Git grammar helpers (pure core)",
        applies_to: &["src/operations/git/cli_parser/"],
        allowed_crate_prefixes: &[
            "crate::operations::git::cli_parser",
            "crate::operations::git::command_policy",
        ],
        excluding: &[],
        forbidden_crate_modules: &[],
        forbidden_extern_crates: PURE_CORE_FORBIDDEN_CRATES,
        forbidden_prefixes: PURE_CORE_FORBIDDEN_PREFIXES,
    },
];

/// Substrings whose presence in non-test code signals filesystem or process IO.
const PURE_CORE_FORBIDDEN_IO_SUBSTRINGS: &[&str] = &[
    ".canonicalize(",
    "std::process::",
    "std::fs::",
    "std::io::",
    "File::open",
    ".exists(",
    ".try_exists(",
    ".metadata(",
    ".symlink_metadata(",
    ".read_dir(",
    ".is_file(",
    ".is_dir(",
    "::now(",
];

/// The stream-adapter layer owns transcript parsing and discovery policy.
///
/// It must not reach upward into daemon orchestration: doing so makes the
/// adapters impossible to reuse without the daemon and gives pure parsing
/// helpers the wrong architectural owner.
const STREAMS_FORBIDDEN_DEPENDENCIES: &[&str] = &["crate::operations::daemon"];

fn rule_applies(rule: &Rule, rel: &str) -> bool {
    if !rule.applies_to.iter().any(|prefix| rel.starts_with(prefix)) {
        return false;
    }
    !rule.excluding.iter().any(|ex| rel.starts_with(ex))
}

/// Expand a single `use` path that may contain a brace group into its concrete
/// member paths.  Only single-level brace groups are supported (e.g.
/// `std::{fs, io}` → `["std::fs", "std::io"]`).  If a nested brace group is
/// detected, this function panics with a message asking the author to split the
/// import — full recursive expansion is not warranted given this codebase's
/// import style.
///
/// `path` must be the target after `use ` (or after `use crate::` for
/// crate-relative paths), already stripped of trailing `;` / whitespace.
fn expand_use_path(path: &str) -> Vec<String> {
    let Some(brace_start) = path.find('{') else {
        return vec![path.to_string()];
    };
    let prefix = &path[..brace_start];
    let Some(brace_end) = path.find('}') else {
        panic!("layer_import_policy: incomplete import `{path}`");
    };
    let inner = &path[brace_start + 1..brace_end];
    if inner.contains('{') {
        panic!(
            "layer_import_policy: nested brace group in `use {path}` is not supported by the \
             policy scanner — please split this import into separate `use` lines so each path \
             can be checked individually."
        );
    }
    inner
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(|member| format!("{prefix}{member}"))
        .collect()
}

/// Expand one complete import statement; nested groups remain unsupported.
fn parse_use_target(statement: &str) -> Option<(bool, &str, Vec<String>)> {
    let statement = statement.trim();
    let target = statement
        .strip_prefix("pub(crate) use ")
        .or_else(|| statement.strip_prefix("pub use "))
        .or_else(|| statement.strip_prefix("use "))?
        .trim_end_matches(';')
        .trim();
    let is_crate = target.starts_with("crate::");
    let first = target
        .strip_prefix("crate::")
        .unwrap_or(target)
        .split([':', ';', '{', ' ', ','])
        .next()?;
    Some((is_crate, first, expand_use_path(target)))
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

/// Returns the portion of `content` before the trailing test module.
///
/// The trailing test module is identified as the last `#[cfg(test)]` that is
/// immediately followed (on the next non-empty line) by `mod `.  If a
/// `#[cfg(test)]` occurs in the file but is NOT followed by `mod ` (e.g. an
/// early `#[cfg(test)] use …`), this function panics with a clear message so
/// that authors cannot silently exempt production code from the IO scan.
fn non_test_content(content: &str) -> &str {
    // Find all occurrences of "#[cfg(test)]" and locate the one that introduces
    // the trailing test module.
    let mut test_module_offset: Option<usize> = None;
    let mut search_start = 0;
    while let Some(rel_offset) = content[search_start..].find("#[cfg(test)]") {
        let offset = search_start + rel_offset;
        // Find the next non-empty line after this marker.
        let after_marker = &content[offset + "#[cfg(test)]".len()..];
        let next_line = after_marker
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        if next_line.trim().starts_with("mod ") {
            test_module_offset = Some(offset);
            // Keep searching: we want the LAST such marker (should be only one,
            // but we want to be safe).
        } else {
            // A #[cfg(test)] that does NOT introduce a mod — it must be an
            // item-level gate. We still need to ensure it's inside the test
            // module we already found, or panic.
            if test_module_offset.is_none() || offset < test_module_offset.unwrap() {
                panic!(
                    "layer_import_policy: found `#[cfg(test)]` at byte offset {offset} that is \
                     NOT followed by `mod …` (it appears to gate an individual item outside the \
                     test module). This would silently exempt production code from the IO scan. \
                     Move this item inside the test module or re-evaluate the policy."
                );
            }
            // It's inside the test module already found — that's fine.
        }
        search_start = offset + 1;
    }
    match test_module_offset {
        Some(idx) => &content[..idx],
        None => content,
    }
}

fn path_has_prefix(path: &str, prefix: &str) -> bool {
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with("::"))
}

fn resolve_relative_path(rel: &str, path: &str) -> String {
    let path = path
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_start_matches("::");
    if !path.starts_with("self::") && !path.starts_with("super::") {
        return path.to_string();
    }
    let mut module: Vec<_> = rel
        .strip_prefix("src/")
        .unwrap()
        .trim_end_matches(".rs")
        .split('/')
        .collect();
    if module.last() == Some(&"mod") {
        module.pop();
    }
    let mut tail = path.strip_prefix("self::").unwrap_or(path);
    while let Some(rest) = tail.strip_prefix("super::") {
        assert!(
            module.pop().is_some(),
            "relative import escapes crate: {rel}: {path}"
        );
        tail = rest;
    }
    format!("crate::{}::{tail}", module.join("::"))
}

fn source_violations(rel: &str, content: &str) -> Vec<String> {
    // A complete use statement wins over its inner paths, so brace prefixes
    // cannot be mistaken for dependencies on the whole parent module.
    static PATHS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\buse\s+[A-Za-z_:][A-Za-z_0-9:*,{}\s]*;|\b(?:crate|self|super|std|tokio|rusqlite|ureq|interprocess|serde_json|rand|tracing|chrono)(?:::[A-Za-z_][A-Za-z_0-9]*)+").unwrap()
    });
    let rules: Vec<_> = RULES
        .iter()
        .filter(|rule| rule_applies(rule, rel))
        .collect();
    if rules.is_empty() {
        return Vec::new();
    }
    let pure = rules
        .iter()
        .any(|rule| !rule.allowed_crate_prefixes.is_empty());
    let content = content
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let production_end = if pure {
        non_test_content(&content).len()
    } else {
        content.find("#[cfg(test)]\nmod ").unwrap_or(content.len())
    };
    let mut violations = Vec::new();
    for matched in PATHS.find_iter(&content) {
        let imported = parse_use_target(matched.as_str());
        let is_import = imported.is_some();
        let after_test_module = matched.start() >= production_end;
        // Keep the original whole-file import check. Relative paths inside
        // inline test modules cannot be resolved from the filename alone.
        if after_test_module && !is_import {
            continue;
        }
        // Preserve the existing import-only scope for persistence/client tests.
        if imported.is_none() && !pure && !rel.starts_with("src/model/") {
            continue;
        }
        if imported.is_none() && rel.starts_with("src/model/repository/") {
            continue;
        }
        let paths =
            imported.map_or_else(|| vec![matched.as_str().to_string()], |(_, _, paths)| paths);
        for path in paths {
            if is_import {
                let words: Vec<_> = path.split_whitespace().collect();
                if !matches!(words.as_slice(), [_] | [_, "as", _]) {
                    violations.push(format!(
                        "{rel}: split comments and imports so `{path}` can be checked"
                    ));
                    continue;
                }
            }
            if after_test_module && (path.starts_with("super::") || path.starts_with("self::")) {
                continue;
            }
            let mut path = resolve_relative_path(rel, &path);
            if is_import {
                path = path
                    .strip_suffix("::self")
                    .or_else(|| path.strip_suffix("::*"))
                    .unwrap_or(&path)
                    .to_string();
            }
            for rule in &rules {
                let is_crate = path.starts_with("crate::");
                let root = path
                    .strip_prefix("crate::")
                    .unwrap_or(&path)
                    .split("::")
                    .next()
                    .unwrap();
                let forbidden_root = if is_crate {
                    rule.forbidden_crate_modules
                } else {
                    rule.forbidden_extern_crates
                };
                let forbidden = matches!(path.as_str(), "crate" | "self" | "super")
                    || forbidden_root.contains(&root)
                    || rule.forbidden_prefixes.iter().any(|prefix| {
                        path_has_prefix(&path, prefix)
                            || (is_import && path_has_prefix(prefix, &path))
                    })
                    || (is_crate
                        && !rule.allowed_crate_prefixes.is_empty()
                        && !rule
                            .allowed_crate_prefixes
                            .iter()
                            .any(|prefix| path_has_prefix(&path, prefix)));
                if forbidden {
                    let line = content[..matched.start()]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                        + 1;
                    violations.push(format!(
                        "{rel}:{line}: {} must not depend on `{path}`",
                        rule.layer
                    ));
                }
            }
        }
    }
    if pure {
        for forbidden in PURE_CORE_FORBIDDEN_IO_SUBSTRINGS {
            if let Some(offset) = content[..production_end].find(forbidden) {
                let line = content[..offset]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                violations.push(format!(
                    "{rel}:{line}: pure core must not contain `{forbidden}`"
                ));
            }
        }
    }
    violations
}

#[test]
fn src_layers_respect_import_direction() {
    let root = repo_root();
    let files = collect_src_files(&root);
    for rule in RULES {
        for prefix in rule.applies_to {
            assert!(
                files.iter().any(|(rel, _)| rel.starts_with(prefix)),
                "layer policy scope {prefix} matches no source files"
            );
        }
    }
    let mut violations = Vec::new();

    for (rel, content) in &files {
        violations.extend(source_violations(rel, content));
    }

    assert!(
        violations.is_empty(),
        "layer import-direction violations:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn stream_adapters_do_not_depend_on_daemon_orchestration() {
    let root = repo_root();
    let files = collect_src_files(&root);
    let mut violations = Vec::new();

    for (rel, content) in files
        .iter()
        .filter(|(rel, _)| rel.starts_with("src/operations/streams/"))
    {
        let production = content
            .rfind("#[cfg(test)]\nmod ")
            .map_or(content.as_str(), |offset| &content[..offset]);
        for forbidden in STREAMS_FORBIDDEN_DEPENDENCIES {
            for (lineno, line) in production.lines().enumerate() {
                if line.contains(forbidden) {
                    violations.push(format!(
                        "{rel}:{}: stream adapters must not depend on `{forbidden}`",
                        lineno + 1,
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "stream adapter dependency violations:\n  {}",
        violations.join("\n  ")
    );
}

#[test]
fn checkpoint_journal_owns_its_storage_boundary() {
    let root = repo_root();
    let journal =
        std::fs::read_to_string(root.join("src/operations/git/repo_storage/checkpoint_journal.rs"))
            .expect("checkpoint journal source should be readable");
    assert!(
        !journal.contains("crate::operations::git::repo_storage"),
        "checkpoint_journal must not import from its parent repo_storage module"
    );
}

#[test]
fn checkpoint_domain_does_not_store_journal_provenance() {
    let root = repo_root();
    let working_log = std::fs::read_to_string(root.join("src/model/working_log.rs"))
        .expect("working-log domain source should be readable");
    assert!(
        !working_log.contains("journal_record_version"),
        "Checkpoint journal provenance belongs to the persistence boundary, not the domain model"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_use_target unit tests ---

    #[test]
    fn parse_use_target_crate_relative_forbidden_prefix() {
        // A crate-relative import that should match a prefix rule like
        // "crate::model::repository".
        let result = parse_use_target("use crate::model::repository::x;");
        let (is_crate, seg, expanded) = result.expect("should parse");
        assert!(is_crate);
        assert_eq!(seg, "model");
        assert_eq!(expanded, vec!["crate::model::repository::x"]);
        // Verify it matches the forbidden prefix.
        assert!(
            expanded
                .iter()
                .any(|p| p.starts_with("crate::model::repository"))
        );
    }

    #[test]
    fn parse_use_target_grouped_import_expansion() {
        // `use std::{fs, io};` should expand to two entries and match
        // "std::fs" and "std::io" prefix rules.
        let result = parse_use_target("use std::{fs, io};");
        let (is_crate, seg, expanded) = result.expect("should parse");
        assert!(!is_crate);
        assert_eq!(seg, "std");
        assert!(
            expanded.contains(&"std::fs".to_string()),
            "expanded = {expanded:?}"
        );
        assert!(
            expanded.contains(&"std::io".to_string()),
            "expanded = {expanded:?}"
        );
        assert!(expanded.iter().any(|p| p.starts_with("std::fs")));
        assert!(expanded.iter().any(|p| p.starts_with("std::io")));
    }

    #[test]
    fn parse_use_target_grouped_crate_import_matches_prefix() {
        // `use crate::model::{repository, domain};` should expand so that
        // "crate::model::repository" matches a forbidden prefix for that entry.
        let result = parse_use_target("use crate::model::{repository, domain};");
        let (is_crate, seg, expanded) = result.expect("should parse");
        assert!(is_crate);
        assert_eq!(seg, "model");
        assert!(
            expanded.contains(&"crate::model::repository".to_string()),
            "expanded = {expanded:?}"
        );
        assert!(
            expanded.contains(&"crate::model::domain".to_string()),
            "expanded = {expanded:?}"
        );
        // Prefix match: "crate::model::repository" starts_with "crate::model::repository".
        assert!(
            expanded
                .iter()
                .any(|p| p.starts_with("crate::model::repository"))
        );
    }
}

#[test]
fn policy_rejects_upward_dependency_syntax() {
    let reducer = "src/operations/daemon/reducer.rs";
    let cases = [
        (reducer, "use crate::operations::git::oid::is_zero_oid;"),
        (reducer, "use crate::{operations::git::oid, model::domain};"),
        (
            reducer,
            "use crate::operations::{\n git::oid,\n daemon::analyzers,\n};",
        ),
        (reducer, "use crate::operations::git::oid as ids;"),
        (
            reducer,
            "fn f() { use crate::{operations::git::oid, model::domain}; }",
        ),
        (reducer, "use crate::model as m;"),
        (reducer, "use crate::model::{self as m, domain};"),
        (reducer, "use crate::model::*;"),
        (reducer, "use std as platform;"),
        (
            reducer,
            "let x = 0; // use the canonical value\nuse crate::operations::git::oid::is_zero_oid;",
        ),
        (reducer, "#[cfg(test)]\nmod tests {}\nuse std::fs::File;"),
        (
            reducer,
            "fn f() { let url = \"http://example\"; crate::operations::git::oid::is_zero_oid(\"0\"); }",
        ),
        (
            reducer,
            "fn f() { crate::operations::git::oid::is_zero_oid(\"0\"); }",
        ),
        (reducer, "use super::super::git::oid;"),
        (
            reducer,
            "fn f() { super::super::git::oid::is_zero_oid(\"0\"); }",
        ),
        (reducer, "use crate::operations::daemon::actor_coordinator;"),
        (
            "src/operations/daemon/analyzers/history.rs",
            "use crate::operations::git::repository::Repository;",
        ),
        (
            "src/model/domain.rs",
            "use crate::model::repository::notes_db;",
        ),
        ("src/model/domain.rs", "use crate::metrics::MetricEvent;"),
        ("src/model/domain.rs", "use super::repository::notes_db;"),
    ];
    let missed: Vec<_> = cases
        .into_iter()
        .filter(|(file, source)| source_violations(file, source).is_empty())
        .collect();
    assert!(
        missed.is_empty(),
        "policy missed forbidden dependencies: {missed:?}"
    );
}

#[test]
fn policy_accepts_audited_pure_dependencies() {
    for (file, source) in [
        (
            "src/operations/daemon/reducer.rs",
            "use crate::model::domain::{\n FamilyState,\n RefChange,\n};",
        ),
        (
            "src/operations/daemon/reducer.rs",
            "use crate::{error::GitAiError, model::domain};",
        ),
        (
            "src/operations/daemon/reducer.rs",
            "fn f() { crate::model::git_oid::is_zero_oid(\"0\"); }",
        ),
        (
            "src/operations/daemon/analyzers/history.rs",
            "use crate::operations::git::cli_parser::explicit_rebase_branch_arg;",
        ),
        (
            "src/operations/daemon/analyzers/generic.rs",
            "use crate::operations::git::command_classification::is_definitely_read_only_command;",
        ),
        (
            "src/operations/daemon/analyzers/generic.rs",
            "use crate::operations::git::command_policy::{is_repo_admin_command, is_transport_command};",
        ),
        (
            "src/operations/git/command_classification.rs",
            "use super::command_policy;",
        ),
        (
            "src/operations/git/cli_parser/rewrite_args.rs",
            "use super::ParsedGitInvocation;",
        ),
        ("src/model/stat_snapshot.rs", "use std::fs::Metadata;"),
        ("src/model/stream_types.rs", "use std::io::BufRead;"),
    ] {
        assert!(
            source_violations(file, source).is_empty(),
            "unexpected violation: {file}: {source}"
        );
    }
}

#[test]
fn policy_rejects_effects_in_pure_git_kernels() {
    for file in [
        "src/operations/git/cli_parser.rs",
        "src/operations/git/cli_parser/rewrite_args.rs",
        "src/operations/git/command_classification.rs",
        "src/operations/git/command_policy.rs",
    ] {
        for source in [
            "use std::fs::File;",
            "fn f() { std::env::var(\"HOME\"); }",
            "fn f() { crate::model::clock::now_secs(); }",
            "use crate::operations::daemon::family_actor;",
            "fn f() { rand::rng(); }",
        ] {
            assert!(
                !source_violations(file, source).is_empty(),
                "policy missed effect: {file}: {source}"
            );
        }
    }
}

#[test]
fn policy_covers_future_analyzer_files() {
    let file = "src/operations/daemon/analyzers/future.rs";
    for source in [
        "fn f(path: &std::path::Path) { path.exists(); }",
        "fn f() { std::time::SystemTime::now(); }",
        "fn f() { crate::model::clock::now_secs(); }",
    ] {
        assert!(
            !source_violations(file, source).is_empty(),
            "new analyzer escaped pure-core policy: {source}"
        );
    }
}
