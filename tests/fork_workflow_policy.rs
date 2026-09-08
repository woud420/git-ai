use std::fs;
use std::path::{Path, PathBuf};

const REVIEW_PROCESS_FILES: &[&str] = &[
    "AGENTS.md",
    "scripts/ai",
    "docs/session-event-attribution-recovery-plan.md",
    "tests/integration/checkpoint_unit.rs",
];
const DEVIN_REVIEW_BOT_NAME: &str = "devin";
const GRAPHITE_NAME: &str = "graphite";
const GRAPHITE_ACTIVE_ROOTS: &[&str] = &[
    "AGENTS.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "Cargo.toml",
    "Makefile",
    "README-nix.md",
    "README.md",
    "agent-support",
    "benches",
    "data-privacy.md",
    "docs",
    "flake.nix",
    "install.ps1",
    "install.sh",
    "lefthook.yml",
    "packaging",
    "scripts",
    "specs",
    "src",
    "tests",
    "uninstall.ps1",
    "uninstall.sh",
    ".cargo",
    ".github",
];
const TASK_RETIRED_PATHS: &[&str] = &["Taskfile.yml"];
const TASK_MAINTAINED_SURFACES: &[&str] = &[
    ".github/actions/setup-gnu-make/action.yml",
    ".github/workflows/e2e-tests.yml",
    ".github/workflows/git-core-compat.yml",
    ".github/workflows/lint-format.yml",
    ".github/workflows/performance-benchmarks.yml",
    ".github/workflows/test.yml",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "Makefile",
    "docs/COVERAGE.md",
    "docs/operations/README.md",
    "flake.nix",
    "lefthook.yml",
    "scripts/ai",
    "tests/windows_ci_workflow_contract.rs",
];
const TASK_RETIRED_FRAGMENTS: &[&str] = &[
    "Taskfile.yml",
    "go-task/setup-task",
    "task build",
    "task check:windows",
    "task coverage",
    "task dev",
    "task doc",
    "task fmt",
    "task format:check",
    "task lint",
    "task test",
];
const REQUIRED_MAKE_TARGETS: &[&str] = &[
    "build",
    "check",
    "check-windows",
    "clean",
    "coverage",
    "coverage-check",
    "coverage-html",
    "coverage-lcov",
    "dev",
    "doc",
    "fmt",
    "format-check",
    "lint",
    "test",
    "test-bats",
    "test-fuzz",
    "test-fuzz-all",
    "test-fuzz-combined",
    "test-fuzz-destructive",
    "test-fuzz-heavy",
    "test-fuzz-marathon",
    "test-fuzz-partial",
    "test-fuzz-squash",
    "test-fuzz-workflow",
];
const REQUIRED_MAKE_INTERFACE: &[&str] = &[
    "CARGO_TEST_ARGS",
    "COVERAGE_THRESHOLD",
    "EXTRA_TEST_BINARY_ARGS",
    "GIT_AI_TEST_SHARED_DAEMON_POOL_SIZE",
    "NO_CAPTURE",
    "MINIMUM_MAKE_VERSION := 4.4.1",
    "NUMBER_OF_PROCESSORS",
    "TEST_FILTER",
    "TEST_THREADS",
    "getconf _NPROCESSORS_ONLN",
    "nproc",
    "scripts/dev.ps1",
    "scripts/dev.sh",
    "sysctl -n hw.ncpu",
];
const GNU_MAKE_SETUP_ACTION: &str = ".github/actions/setup-gnu-make/action.yml";
const GNU_MAKE_SETUP_USE: &str = "uses: ./.github/actions/setup-gnu-make";
const GNU_MAKE_WORKFLOWS: &[(&str, usize)] = &[
    (".github/workflows/e2e-tests.yml", 1),
    (".github/workflows/lint-format.yml", 3),
    (".github/workflows/test.yml", 1),
];
const GRAPHITE_SCAN_EXCEPTIONS: &[&str] = &[
    "docs/pull-rebase-hardening-worklog-2026-06-21.md",
    "tests/fork_workflow_policy.rs",
    "tests/fork_workflow_policy/workflow_tools.rs",
];
const GRAPHITE_SCAN_IGNORED_DIRECTORIES: &[&str] =
    &[".gradle", "build", "dist", "node_modules", "out", "target"];
const GRAPHITE_RETIRED_PATHS: &[&str] = &[
    ".github/workflows/graphite-compatibility.yml",
    "src/operations/authorship/rewrite/split_by_file.rs",
    "tests/integration/graphite.rs",
];
const GRAPHITE_RETIRED_WIRING: &[(&str, &str)] = &[
    (".github/workflows/coverage.yml", "--skip graphite::"),
    (".github/workflows/test.yml", "graphite::"),
    (
        "src/operations/authorship/rewrite/mod.rs",
        "mod split_by_file;",
    ),
    (
        "src/operations/authorship/rewrite/range_diff.rs",
        "derive_split_commit_mappings",
    ),
    ("tests/integration/main.rs", "mod graphite;"),
];
const BUNDLED_SKILL_FILES: &[&str] = &[
    ".agents/skills/ask/SKILL.md",
    ".agents/skills/git-ai-search/SKILL.md",
    ".agents/skills/prompt-analysis/SKILL.md",
];
const RETIRED_SKILL_COMMANDS: &[&str] = &["git-ai search", "git-ai continue", "git-ai prompts"];
const ACTIVE_DISTRIBUTION_FILES: &[&str] = &[
    "README-nix.md",
    "CONTRIBUTING.md",
    "agent-support/intellij/README.md",
    "agent-support/intellij/gradle.properties",
    "agent-support/intellij/src/main/kotlin/org/jetbrains/plugins/template/services/GitAiService.kt",
    "agent-support/opencode/README.md",
    "agent-support/opencode/git-ai.ts",
    "agent-support/visualstudio/README.md",
    "agent-support/visualstudio/src/GitAiVS/GitAiPackage.cs",
    "agent-support/visualstudio/src/GitAiVS/Services/BinaryResolver.cs",
    "agent-support/visualstudio/src/GitAiVS/source.extension.vsixmanifest",
    "agent-support/vscode/README.md",
    "agent-support/vscode/package.json",
    "agent-support/vscode/src/ai-edit-manager.ts",
    "agent-support/vscode/src/blame-service.ts",
    "agent-support/vscode/src/consts.ts",
    "docs/operations/README.md",
    "flake.nix",
];
const STALE_DISTRIBUTION_FRAGMENTS: &[&str] = &[
    "github:acunniffe/git-ai",
    "github.com/acunniffe/git-ai",
    "github.com/git-ai-project/git-ai",
    "install.usegitai.com",
    "discord.gg/XJStYvkb5U",
    "calendly.com/d/cxjh-z79-ktm",
    "Visit https://usegitai.com to install it",
    "<MoreInfo>https://usegitai.com</MoreInfo>",
    "Learn more at [usegitai.com]",
];

fn coverage_defaults_agree(makefile: &str, workflow: &str) -> bool {
    let scalar = |source: &str, prefix: &str| {
        source.lines().find_map(|line| {
            line.trim()
                .strip_prefix(prefix)?
                .split('#')
                .next()?
                .trim()
                .parse::<u32>()
                .ok()
        })
    };
    let make_default = scalar(makefile, "COVERAGE_THRESHOLD ?=");
    let ci_default = scalar(workflow, "COVERAGE_THRESHOLD:");
    matches!((make_default, ci_default), (Some(a), Some(b)) if a <= 100 && a == b)
}

fn assert_live_architecture_references(root: &Path, inventory: &str, ownership: &str) {
    for (relative, contents) in [
        ("docs/architecture/inventory.md", inventory),
        ("docs/architecture/state-ownership.md", ownership),
    ] {
        assert!(
            !contents.contains(".rs:"),
            "{relative} must cite stable Rust modules or symbols, not line offsets"
        );
    }

    for source_path in [
        "src/config/mod.rs",
        "src/model/repository/notes_db.rs",
        "src/operations/daemon/self_check.rs",
        "src/operations/commands/diff.rs",
    ] {
        assert!(
            root.join(source_path).is_file(),
            "architecture source path must resolve: {source_path}"
        );
        let documented_path = source_path.trim_start_matches("src/");
        assert!(
            inventory.contains(documented_path) || ownership.contains(documented_path),
            "live architecture docs omit stable source path `{documented_path}`"
        );
    }
    for symbol in [
        "`CONFIG`",
        "`AUTHOR_CONFIG_CACHE`",
        "`NOTES_DB`",
        "`DAEMON_PROCESS_ACTIVE`",
        "`SystemGitBackend.alias_cache`",
        "`DiffHunk`",
    ] {
        assert!(
            inventory.contains(symbol) || ownership.contains(symbol),
            "live architecture docs omit stable symbol `{symbol}`"
        );
    }
}

fn is_repository_text_file(path: &Path) -> bool {
    path.extension().is_none()
        || matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some(
                "bat"
                    | "bats"
                    | "cs"
                    | "csproj"
                    | "json"
                    | "kt"
                    | "kts"
                    | "md"
                    | "mjs"
                    | "nix"
                    | "properties"
                    | "ps1"
                    | "py"
                    | "rs"
                    | "sh"
                    | "sln"
                    | "svg"
                    | "toml"
                    | "ts"
                    | "tsx"
                    | "txt"
                    | "wxs"
                    | "xml"
                    | "yml"
                    | "yaml"
            )
        )
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let file_type = entry.file_type().expect("failed to read entry file type");
        let path = entry.path();
        if file_type.is_dir() {
            if path.file_name().is_some_and(|name| {
                GRAPHITE_SCAN_IGNORED_DIRECTORIES
                    .iter()
                    .any(|ignored| name == *ignored)
            }) {
                continue;
            }
            collect_files(&path, files);
        } else if file_type.is_file() {
            files.push(path);
        }
    }
}

fn collect_markdown_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("failed to read directory entry");
        let path = entry.path();
        if entry
            .file_type()
            .expect("failed to read entry file type")
            .is_dir()
        {
            if !GRAPHITE_SCAN_IGNORED_DIRECTORIES
                .iter()
                .any(|ignored| path.file_name().is_some_and(|name| name == *ignored))
            {
                collect_markdown_files(&path, files);
            }
        } else if path.extension().is_some_and(|extension| extension == "md") {
            files.push(path);
        }
    }
}

fn local_markdown_targets(contents: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    for line in contents.lines() {
        let mut remainder = line;
        while let Some(start) = remainder.find("](") {
            remainder = &remainder[start + 2..];
            let Some(end) = remainder.find(')') else {
                break;
            };
            let target = remainder[..end].trim().trim_matches(['<', '>']);
            if is_local_markdown_target(target) {
                targets.push(target);
            }
            remainder = &remainder[end + 1..];
        }

        if line.starts_with('[')
            && let Some((_, target)) = line.split_once("]: ")
        {
            let target = target.trim().trim_matches(['<', '>']);
            if is_local_markdown_target(target) {
                targets.push(target);
            }
        }
    }
    targets
}

fn is_local_markdown_target(target: &str) -> bool {
    !target.is_empty()
        && !target.starts_with('#')
        && !target.starts_with("http://")
        && !target.starts_with("https://")
        && !target.starts_with("mailto:")
}

#[path = "fork_workflow_policy/architecture_docs.rs"]
mod architecture_docs;
#[path = "fork_workflow_policy/build_contracts.rs"]
mod build_contracts;
#[path = "fork_workflow_policy/distribution.rs"]
mod distribution;
#[path = "fork_workflow_policy/editor_integrations.rs"]
mod editor_integrations;
#[path = "fork_workflow_policy/privacy_and_evidence.rs"]
mod privacy_and_evidence;
#[path = "fork_workflow_policy/workflow_tools.rs"]
mod workflow_tools;
