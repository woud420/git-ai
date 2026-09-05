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
    ".github/workflows/e2e-tests.yml",
    ".github/workflows/git-core-compat.yml",
    ".github/workflows/lint-format.yml",
    ".github/workflows/performance-benchmarks.yml",
    ".github/workflows/test.yml",
    "AGENTS.md",
    "CONTRIBUTING.md",
    "Makefile",
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
    "NUMBER_OF_PROCESSORS",
    "TEST_FILTER",
    "TEST_THREADS",
    "getconf _NPROCESSORS_ONLN",
    "nproc",
    "scripts/dev.ps1",
    "scripts/dev.sh",
    "sysctl -n hw.ncpu",
];
const GRAPHITE_SCAN_EXCEPTIONS: &[&str] = &[
    "docs/pull-rebase-hardening-worklog-2026-06-21.md",
    "tests/fork_workflow_policy.rs",
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

// Regression coverage for ENG-352.
#[test]
fn eng_352_active_fork_surfaces_do_not_maintain_graphite_compatibility() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for relative in GRAPHITE_RETIRED_PATHS {
        if root.join(relative).exists() {
            violations.push(format!("{relative} (retired path still exists)"));
        }
    }
    for (relative, fragment) in GRAPHITE_RETIRED_WIRING {
        let path = root.join(relative);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if contents.contains(fragment) {
            violations.push(format!("{relative} (retired wiring remains)"));
        }
    }

    let mut files = Vec::new();
    for relative in GRAPHITE_ACTIVE_ROOTS {
        let path = root.join(relative);
        if path.is_dir() {
            collect_files(&path, &mut files);
        } else {
            files.push(path);
        }
    }
    files.sort();
    files.dedup();

    for path in files {
        let relative = path.strip_prefix(root).unwrap_or(&path);
        let relative_text = relative.to_string_lossy();
        if GRAPHITE_SCAN_EXCEPTIONS
            .iter()
            .any(|exception| relative == Path::new(exception))
            || !is_repository_text_file(&path)
        {
            continue;
        }

        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        if relative_text.to_ascii_lowercase().contains(GRAPHITE_NAME)
            || contents.to_ascii_lowercase().contains(GRAPHITE_NAME)
        {
            violations.push(relative_text.into_owned());
        }
    }

    assert!(
        violations.is_empty(),
        "active fork surfaces still maintain Graphite compatibility:\n{}",
        violations.join("\n")
    );
}

#[test]
fn eng_286_make_is_the_only_maintained_command_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let makefile_path = root.join("Makefile");
    let makefile = fs::read_to_string(&makefile_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", makefile_path.display()));

    for relative in TASK_RETIRED_PATHS {
        assert!(
            !root.join(relative).exists(),
            "retired Task path still exists: {relative}"
        );
    }

    let declared_targets = makefile
        .lines()
        .filter(|line| !line.starts_with('\t') && !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once(':').map(|(targets, _)| targets))
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>();
    for target in REQUIRED_MAKE_TARGETS {
        assert!(
            declared_targets.contains(target),
            "Makefile is missing required target `{target}`"
        );
    }
    for fragment in REQUIRED_MAKE_INTERFACE {
        assert!(
            makefile.contains(fragment),
            "Makefile is missing required interface fragment `{fragment}`"
        );
    }

    let check_steps = ["$(MAKE) lint", "$(MAKE) format-check", "$(MAKE) test"];
    let mut previous = 0;
    for step in check_steps {
        let index = makefile
            .find(step)
            .unwrap_or_else(|| panic!("make check is missing sequential step `{step}`"));
        assert!(index >= previous, "make check steps are out of order");
        previous = index;
    }

    let mut violations = Vec::new();
    for relative in TASK_MAINTAINED_SURFACES {
        let path = root.join(relative);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for fragment in TASK_RETIRED_FRAGMENTS {
            if contents.contains(fragment) {
                violations.push(format!("{relative}: `{fragment}`"));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "maintained surfaces still depend on Task:\n{}",
        violations.join("\n")
    );
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
