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

#[test]
fn eng_286_requires_and_provisions_gnu_make_4_4_1() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let setup_path = root.join(GNU_MAKE_SETUP_ACTION);
    let setup = fs::read_to_string(&setup_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", setup_path.display()));

    for fragment in [
        "GNU_MAKE_VERSION=4.4.1",
        "dd16fb1d67bfab79a72f5e8390735c49e3e8e70b4945a15ab1f81ddb78658fb3",
        "brew install make",
        "choco install make --version=4.4.1 --no-progress --yes",
        "GNU Make 4.4.1",
    ] {
        assert!(
            setup.contains(fragment),
            "GNU Make setup action is missing `{fragment}`"
        );
    }

    let source_directory_index = setup
        .find("cd \"$source_dir\"")
        .expect("Linux setup must enter the extracted source directory");
    let configure_index = setup
        .find("./configure --prefix=\"$prefix\"")
        .expect("Linux setup must configure from the extracted source directory");
    assert!(
        source_directory_index < configure_index,
        "Linux setup must enter the extracted source directory before configuring"
    );

    for (relative, expected_uses) in GNU_MAKE_WORKFLOWS {
        let path = root.join(relative);
        let workflow = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        assert_eq!(
            workflow.matches(GNU_MAKE_SETUP_USE).count(),
            *expected_uses,
            "{relative} must set up GNU Make once per Make-running job"
        );
    }

    let contributing =
        fs::read_to_string(root.join("CONTRIBUTING.md")).expect("CONTRIBUTING.md must be readable");
    assert!(
        contributing.contains("GNU Make 4.4.1 or newer"),
        "contributor prerequisites must require GNU Make 4.4.1 or newer"
    );
    assert!(
        contributing.contains("$(brew --prefix make)/libexec/gnubin"),
        "macOS setup must document Homebrew's gnubin path"
    );
}

#[test]
fn eng_372_bundled_skills_reference_supported_commands() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut combined = String::new();
    let mut violations = Vec::new();

    for relative in BUNDLED_SKILL_FILES {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for command in RETIRED_SKILL_COMMANDS {
            if contents.contains(command) {
                violations.push(format!("{relative}: `{command}`"));
            }
        }
        combined.push_str(&contents);
    }

    assert!(
        violations.is_empty(),
        "bundled skills still invoke retired commands:\n{}",
        violations.join("\n")
    );
    for command in [
        "git-ai blame",
        "git-ai show ",
        "git-ai show-prompt",
        "git-ai analyze",
    ] {
        assert!(
            combined.contains(command),
            "bundled skills do not document supported command `{command}`"
        );
    }
}

#[test]
fn eng_373_active_distribution_guidance_is_fork_local() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for relative in ACTIVE_DISTRIBUTION_FILES {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for fragment in STALE_DISTRIBUTION_FRAGMENTS {
            if contents.contains(fragment) {
                violations.push(format!("{relative}: `{fragment}`"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "active distribution surfaces still route users away from the fork:\n{}",
        violations.join("\n")
    );

    let operations = fs::read_to_string(root.join("docs/operations/README.md"))
        .expect("operations guide must be readable");
    for required in [
        "has not published tags or release artifacts",
        "macOS Intel",
        "macOS Apple Silicon",
        "macOS universal",
    ] {
        assert!(
            operations.contains(required),
            "operations guide is missing release-state fact `{required}`"
        );
    }
}

#[test]
fn eng_374_nix_options_match_runtime_configuration() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let flake = fs::read_to_string(root.join("flake.nix")).expect("flake.nix must be readable");
    let readme =
        fs::read_to_string(root.join("README-nix.md")).expect("README-nix.md must be readable");

    for retired in [
        "rewriteStash",
        "rewrite_stash = cfg.settings.featureFlags",
        "gitHooksEnabled",
        "git_hooks_enabled = cfg.settings.featureFlags",
        "gitHooksExternallyManaged",
        "git_hooks_externally_managed = cfg.settings.featureFlags",
    ] {
        assert!(
            !flake.contains(retired),
            "Nix module still exposes retired feature flag `{retired}`"
        );
    }

    assert_eq!(
        flake
            .matches("allowed_repositories = cfg.settings.allowRepositories;")
            .count(),
        2,
        "both Nix modules must emit the canonical allowlist key"
    );
    assert_eq!(
        flake
            .matches("If empty or null, no repositories are allowed.")
            .count(),
        2,
        "both Nix modules must document deny-all allowlist semantics"
    );

    for (option, key) in [
        ("authKeyring", "auth_keyring"),
        ("transcriptStreaming", "transcript_streaming"),
        ("transcriptSweep", "transcript_sweep"),
        ("checkpointDebugLog", "checkpoint_debug_log"),
        ("bashCheckpointsV2", "bash_checkpoints_v2"),
        ("daemonLogUpload", "daemon_log_upload"),
        ("rewriteMetricsEvents", "rewrite_metrics_events"),
    ] {
        assert_eq!(
            flake.matches(&format!("{option} = mkOption")).count(),
            2,
            "both Nix modules must type current option `{option}`"
        );
        assert_eq!(
            flake
                .matches(&format!("{key} = cfg.settings.featureFlags.{option};"))
                .count(),
            2,
            "both Nix modules must serialize current option `{option}`"
        );
        assert!(
            readme.contains(&format!("`featureFlags.{option}`")),
            "Nix README must list current option `featureFlags.{option}`"
        );
    }

    assert!(
        readme.contains("settings.allowRepositories"),
        "Nix README must show an explicit repository opt-in"
    );
}

#[test]
fn eng_375_package_metadata_declares_apache_2_0() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml must be readable");
    let flake = fs::read_to_string(root.join("flake.nix")).expect("flake.nix must be readable");
    let intellij = fs::read_to_string(root.join("agent-support/intellij/README.md"))
        .expect("IntelliJ README must be readable");
    let license = fs::read_to_string(root.join("LICENSE")).expect("LICENSE must be readable");

    assert!(
        cargo.contains("license = \"Apache-2.0\""),
        "Cargo package metadata must declare Apache-2.0"
    );
    assert!(
        flake.contains("license = licenses.asl20;"),
        "Nix package metadata must use the Apache-2.0 license value"
    );
    assert!(
        intellij.contains("[Apache License 2.0](LICENSE)"),
        "IntelliJ README must describe its bundled Apache-2.0 license"
    );
    assert!(
        license.contains("Apache License") && license.contains("Version 2.0, January 2004"),
        "canonical LICENSE must remain Apache License 2.0 text"
    );
}

#[test]
fn eng_376_privacy_contract_is_fork_local_and_backend_aware() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let privacy =
        fs::read_to_string(root.join("data-privacy.md")).expect("privacy guide must be readable");

    for inherited_claim in [
        "writes attribution data into your local git repository",
        "See our ",
        "Git AI Cloud (Personal Dashboards)",
        "Git AI for Teams and Enterprise",
        "all data is sent only to your team's Git AI instance",
        "no code, prompts, or agent usage data is ever sent to Git AI",
        "https://usegitai.com/privacy-policy",
        "https://trust.usegitai.com",
        "https://github.com/git-ai-project/self-hosted",
    ] {
        assert!(
            !privacy.contains(inherited_claim),
            "privacy guide still makes inherited operator claim `{inherited_claim}`"
        );
    }

    for required in [
        "allowed_repositories",
        "~/.git-ai/internal/notes-db",
        "refs/notes/ai",
        "notes_backend.kind = http",
        "prompt_storage",
        "telemetry_enterprise_dsn",
        "This fork does not operate",
        "Version checks and automatic updates",
    ] {
        assert!(
            privacy.contains(required),
            "privacy guide is missing current boundary `{required}`"
        );
    }
}

#[test]
fn eng_377_telemetry_contract_matches_current_storage_and_workers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let summary = fs::read_to_string(root.join("docs/contracts/telemetry-streams-summary.md"))
        .expect("telemetry summary must be readable");
    let examples = fs::read_to_string(root.join("docs/contracts/telemetry-examples.md"))
        .expect("telemetry examples must be readable");
    let index = fs::read_to_string(root.join("docs/contracts/README.md"))
        .expect("contracts index must be readable");
    let index = index.replace("\r\n", "\n");
    let persistence = fs::read_to_string(root.join("docs/contracts/persistence-model.md"))
        .expect("persistence contract must be readable");
    let changelog =
        fs::read_to_string(root.join("CHANGELOG.md")).expect("changelog must be readable");

    for stale in [
        "src/transcripts/",
        "src/daemon/transcript_worker.rs",
        "TranscriptsDatabase",
        "TranscriptWorker",
        "~/.git-ai/transcripts.db",
        "~/.git-ai/metrics.db",
        "processing_stats",
        "1-second",
        "Automatic migration from internal_db",
        "can be deleted safely",
    ] {
        assert!(
            !summary.contains(stale),
            "telemetry summary still contains obsolete claim `{stale}`"
        );
    }

    for required in [
        "~/.git-ai/internal/transcripts-db",
        "~/.git-ai/internal/metrics-db",
        "tracked_streams",
        "StreamWorker",
        "src/operations/daemon/stream_worker.rs",
        "src/operations/daemon/telemetry_worker/",
        "src/operations/streams/",
        "src/model/repository/streams_db.rs",
        "src/model/repository/metrics_db/",
        "allowed_repositories",
        "transcript_streaming",
        "transcript_sweep",
    ] {
        assert!(
            summary.contains(required),
            "telemetry summary is missing current contract fact `{required}`"
        );
    }

    assert!(
        persistence.contains("~/.git-ai/internal/transcripts-db"),
        "persistence contract must name the database path opened by the daemon"
    );
    assert!(
        !persistence.contains("~/.git-ai/internal/streams-db"),
        "persistence contract still names the planned, unopened streams-db path"
    );
    assert!(
        examples.contains("src/operations/daemon/rewrite_metrics.rs"),
        "telemetry examples must reference the current rewrite metrics module"
    );
    assert!(
        !examples.contains("src/daemon/rewrite_metrics.rs"),
        "telemetry examples still reference the pre-decomposition daemon path"
    );
    assert!(
        index.contains("current runtime\n  contract for stream cursors"),
        "contracts index must identify the telemetry summary as a current runtime contract"
    );

    for stale in [
        "New `transcripts.db` database",
        "Long-lived `TranscriptWorker`",
        "automatically migrate to `transcripts.db`",
        "1-second polling interval",
        "`internal_db` module: Now deprecated",
    ] {
        assert!(
            !changelog.contains(stale),
            "Unreleased changelog still contains abandoned telemetry claim `{stale}`"
        );
    }

    for contract in [&summary, &examples] {
        for source_path in contract.split('`').skip(1).step_by(2) {
            if !source_path.starts_with("src/") {
                continue;
            }
            let source_path = source_path
                .split_once(':')
                .map_or(source_path, |(path, _)| path);
            assert!(
                root.join(source_path).exists(),
                "telemetry contract references missing source path `{source_path}`"
            );
        }
    }
}

#[test]
fn eng_378_active_docs_distinguish_untracked_and_known_human_evidence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md must be readable");
    let opencode = fs::read_to_string(root.join("agent-support/opencode/README.md"))
        .expect("OpenCode README must be readable");
    let visual_studio = fs::read_to_string(root.join("agent-support/visualstudio/DESIGN.md"))
        .expect("Visual Studio design must be readable");
    let checkpoint = fs::read_to_string(root.join("docs/contracts/checkpoint-interface.md"))
        .expect("checkpoint contract must be readable");

    for stale in [
        "Changes caught by these checkpoints do get explicit attestations",
        "mark code changes as either human or AI-authored",
        "human checkpoint before AI edits",
        "marking any intermediate changes as human-authored",
        "mark any changes since the last checkpoint as human-authored",
        "No match or MEDIUM confidence (human edit)",
        "send a human checkpoint with pre-edit content",
        "Human before_edit / AI after_edit checkpoint pairs",
    ] {
        assert!(
            ![&agents, &opencode, &visual_studio, &checkpoint]
                .iter()
                .any(|contents| contents.contains(stale)),
            "active checkpoint documentation still contains contradictory phrase `{stale}`"
        );
    }

    for (name, contents) in [
        ("AGENTS.md", &agents),
        ("OpenCode README", &opencode),
        ("Visual Studio design", &visual_studio),
        ("checkpoint contract", &checkpoint),
    ] {
        assert!(
            contents.contains("untracked"),
            "{name} must describe the compatibility checkpoint as untracked"
        );
        assert!(
            contents.contains("known_human"),
            "{name} must reserve known-human attribution for `known_human` evidence"
        );
    }

    assert!(
        agents.contains("remain unattested") && checkpoint.contains("remain unattested"),
        "AGENTS.md and the checkpoint contract must agree that untracked lines have no attestation"
    );
}

#[test]
fn eng_379_active_docs_use_the_configured_authorship_backend() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md must be readable");
    let rewrite = fs::read_to_string(root.join("docs/architecture/rewrite-ops-spec.md"))
        .expect("rewrite spec must be readable");
    let ingestion =
        fs::read_to_string(root.join("docs/architecture/daemon-trace2-ingestion-spec.md"))
            .expect("ingestion spec must be readable");
    let intellij = fs::read_to_string(root.join("agent-support/intellij/README.md"))
        .expect("IntelliJ README must be readable");
    let pi = fs::read_to_string(root.join("agent-support/pi/README.md"))
        .expect("Pi README must be readable");
    let visual_studio = fs::read_to_string(root.join("agent-support/visualstudio/DESIGN.md"))
        .expect("Visual Studio design must be readable");
    let persistence = fs::read_to_string(root.join("docs/contracts/persistence-model.md"))
        .expect("persistence contract must be readable");

    for stale in [
        "stores it as a Git Note under `refs/notes/ai`",
        "saved to Git Notes",
        "│ Git Notes   │",
        "**Authorship notes** (`refs/notes/ai`, one note per commit)",
    ] {
        assert!(
            ![&agents, &rewrite, &intellij, &pi, &visual_studio]
                .iter()
                .any(|contents| contents.contains(stale)),
            "active documentation still assumes the Git Notes backend: `{stale}`"
        );
    }

    for required in [
        "notes_backend.kind",
        "notes_api::read_notes_batch",
        "notes_api::write_notes_batch",
        "`sqlite` (production default)",
        "`git_notes`",
        "`http`",
        "persistence-model.md",
    ] {
        assert!(
            rewrite.contains(required),
            "rewrite spec is missing backend abstraction fact `{required}`"
        );
    }

    assert!(
        pi.contains("git-ai show HEAD")
            && pi.contains("Raw `git notes --ref=ai show HEAD` inspection applies only"),
        "Pi troubleshooting must inspect attribution through the backend-aware CLI"
    );
    assert!(
        visual_studio.contains("Configured authorship backend")
            && visual_studio.contains("git-ai log"),
        "Visual Studio docs must show backend-neutral persistence and inspection"
    );
    assert!(
        ingestion.contains("Git Notes sync") && ingestion.contains("`git_notes` backend"),
        "ingestion spec must scope ref synchronization to its Git Notes role"
    );
    for backend in [
        "sqlite backend, default",
        "git_notes backend, opt-in",
        "http backend, opt-in",
    ] {
        assert!(
            persistence.contains(backend),
            "persistence contract is missing authority row `{backend}`"
        );
    }
}

#[test]
fn eng_380_visual_studio_docs_match_install_and_detection_behavior() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/visualstudio/README.md"))
        .expect("Visual Studio README must be readable");
    let design = fs::read_to_string(root.join("agent-support/visualstudio/DESIGN.md"))
        .expect("Visual Studio design must be readable");
    let detector = fs::read_to_string(
        root.join("agent-support/visualstudio/src/GitAiVS/Detection/CopilotEditDetector.cs"),
    )
    .expect("Visual Studio detector must be readable");

    for required in [
        "Experimental support",
        "`git-ai install-hooks` skips Visual Studio",
        "`git-ai install-hooks --visual-studio-extension`",
        "not download or install a VSIX",
        "Extensions > Manage Extensions",
        "Copilot Chat edits",
        "Inline completions",
    ] {
        assert!(
            readme.contains(required),
            "Visual Studio README is missing lifecycle boundary `{required}`"
        );
    }

    assert!(
        readme.contains("agent-support/visualstudio/src/GitAiVS/GitAiVS.csproj")
            && readme.contains("src/GitAiVS/bin/Release/")
            && !readme.contains("GitAiVS.sln"),
        "Visual Studio build instructions must name files and output paths that exist"
    );

    for required in [
        "src/operations/mdm/agents/visual_studio.rs",
        "Chat edits are the only currently evidenced AI detection path",
        "completions are not attributed",
        "`install_vsix()` returns `false`",
    ] {
        assert!(
            design.contains(required),
            "Visual Studio design is missing implementation boundary `{required}`"
        );
    }

    for stale in [
        "Auto-install via `git ai install-hooks`",
        "Stack trace detection for GitHub Copilot (inline + chat)",
        "stack trace analysis proved sufficient",
        "**File**: `src/mdm/agents/visual_studio.rs`",
        "Implementation.Copilot.*",
    ] {
        assert!(
            !design.contains(stale),
            "Visual Studio design retains unsupported claim `{stale}`"
        );
    }

    for prefix in [
        "GitHub.Copilot",
        "Microsoft.VisualStudio.Copilot",
        "Microsoft.VisualStudio.Conversations.UI.Internal.Copilot",
    ] {
        assert!(
            detector.contains(prefix) && design.contains(prefix),
            "Visual Studio design and detector disagree on prefix `{prefix}`"
        );
    }
}

#[test]
fn eng_381_docs_distinguish_serialization_from_storage_profile_compliance() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).expect("README must be readable");
    let contracts = fs::read_to_string(root.join("docs/contracts/README.md"))
        .expect("contracts index must be readable");
    let standard = fs::read_to_string(root.join("specs/git_ai_standard_v3.0.0.md"))
        .expect("authorship standard must be readable");
    let persistence = fs::read_to_string(root.join("docs/contracts/persistence-model.md"))
        .expect("persistence contract must be readable");

    assert!(
        readme.matches("serialization format").count() >= 2
            && readme.matches("storage profile").count() >= 2,
        "README must qualify both authorship-standard compatibility claims"
    );
    assert!(
        readme.contains("specs/git_ai_standard_v3.0.0.md")
            && contracts.contains("../../specs/git_ai_standard_v3.0.0.md"),
        "fork documentation must preserve local links to the upstream standard"
    );
    assert!(
        contracts.contains("serialization format")
            && contracts.contains("Git Notes storage profile")
            && contracts.contains("persistence-model.md"),
        "contracts index must distinguish the record format from storage authority"
    );

    for stale in [
        "Authorship records follow the repository's",
        "authorship metadata using the repository's",
        "the Git AI note format standard",
    ] {
        assert!(
            !readme.contains(stale) && !contracts.contains(stale),
            "active fork documentation overstates standard compliance: `{stale}`"
        );
    }

    for normative in [
        "Authorship logs MUST be stored under the `refs/notes/ai` namespace",
        "considered compliant with this standard if it also attached AI Authorship Logs with Git Notes",
        "https://github.com/git-ai-project/git-ai",
    ] {
        assert!(
            standard.contains(normative),
            "the vendored upstream standard lost normative or origin text `{normative}`"
        );
    }
    assert!(
        persistence.contains("sqlite backend, default")
            && persistence.contains("`refs/notes/ai` (only if exported/migrated)"),
        "the compatibility wording must remain grounded in the persistence contract"
    );
}

#[test]
fn eng_382_current_architecture_source_paths_resolve() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let paths = [
        "AGENTS.md",
        "docs/architecture/README.md",
        "docs/architecture/daemon-trace2-ingestion-spec.md",
        "docs/architecture/rewrite-ops-spec.md",
        "docs/architecture/inventory.md",
        "docs/contracts/telemetry-examples.md",
    ];
    let docs = paths
        .iter()
        .map(|path| {
            fs::read_to_string(root.join(path))
                .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    for stale in [
        "we do NOT wrap git",
        "nothing wraps git",
        "with pre/post hooks per subcommand",
        "src/operations/commands/checkpoint_agent/agent_presets.rs",
        "src/operations/daemon/trace_normalizer.rs",
        "src/operations/daemon/ref_cursor.rs",
        "src/operations/authorship/rewrite.rs",
        "src/error.rs",
        "tests/integration/repos/test_repo.rs",
        "schema in `src/operations/daemon/rewrite_metrics.rs`",
    ] {
        assert!(
            !docs.contains(stale),
            "active architecture documentation retains stale reference `{stale}`"
        );
    }

    for source_path in [
        "src/cli/git_handlers.rs",
        "src/operations/commands/checkpoint_agent/presets/mod.rs",
        "src/operations/daemon/socket_listeners.rs",
        "src/operations/daemon/actor_coordinator_ingest.rs",
        "src/operations/daemon/trace_normalizer/mod.rs",
        "src/operations/daemon/ref_cursor/enrichment.rs",
        "src/operations/daemon/analyzers/history.rs",
        "src/model/domain.rs",
        "src/operations/authorship/rewrite/mod.rs",
        "src/model/hunk_shift.rs",
        "src/error/mod.rs",
        "tests/integration/repos/test_repo/mod.rs",
        "src/model/metrics/events/rewrite_committed.rs",
        "src/operations/daemon/rewrite_metrics.rs",
    ] {
        assert!(
            root.join(source_path).is_file(),
            "documented source path does not resolve: `{source_path}`"
        );
        assert!(
            docs.contains(source_path),
            "architecture documentation omits canonical source path `{source_path}`"
        );
    }
}

#[test]
fn eng_383_checkpoint_preset_registry_drives_help_and_contract() {
    use git_ai::operations::commands::checkpoint_agent::presets::{
        checkpoint_preset_help, human_checkpoint_preset_names, resolve_preset,
        supported_agent_preset_names, test_checkpoint_preset_names,
    };

    let production = supported_agent_preset_names().collect::<Vec<_>>();
    assert_eq!(
        production,
        [
            "claude",
            "cline",
            "codex",
            "gemini",
            "windsurf",
            "continue-cli",
            "cursor",
            "cursor-background",
            "github-copilot",
            "amp",
            "ai_tab",
            "firebender",
            "agent-v1",
            "droid",
            "opencode",
            "pi",
        ]
    );

    let human = human_checkpoint_preset_names().collect::<Vec<_>>();
    let test_only = test_checkpoint_preset_names().collect::<Vec<_>>();
    assert_eq!(human, ["human", "known_human"]);
    assert_eq!(test_only, ["mock_ai", "mock_known_human"]);

    let help = checkpoint_preset_help();
    let contract = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/contracts/checkpoint-interface.md"),
    )
    .expect("checkpoint contract must be readable");

    for preset in production.iter().chain(&human).chain(&test_only) {
        assert!(
            resolve_preset(preset).is_ok(),
            "registered preset `{preset}` must resolve"
        );
        assert!(
            help.contains(preset),
            "top-level help omits preset `{preset}`"
        );
        assert!(
            contract.contains(preset),
            "checkpoint contract omits preset `{preset}`"
        );
    }
    for heading in ["Agent presets:", "Human checkpoints:", "Test presets:"] {
        assert!(
            help.contains(heading),
            "checkpoint help omits category `{heading}`"
        );
    }
    for heading in [
        "Production agent presets",
        "Compatibility and human-evidence presets",
        "Test-only presets",
    ] {
        assert!(
            contract.contains(heading),
            "checkpoint contract omits category `{heading}`"
        );
    }
}

#[test]
fn eng_384_intellij_docs_describe_the_plugin_instead_of_the_template() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let docs_root = root.join("agent-support/intellij");
    let readme =
        fs::read_to_string(docs_root.join("README.md")).expect("IntelliJ README must be readable");
    let changelog = fs::read_to_string(docs_root.join("CHANGELOG.md"))
        .expect("IntelliJ changelog must be readable");
    let conduct = fs::read_to_string(docs_root.join("CODE_OF_CONDUCT.md"))
        .expect("IntelliJ conduct note must be readable");
    let detector = fs::read_to_string(
        docs_root
            .join("src/main/kotlin/org/jetbrains/plugins/template/listener/StackTraceAnalyzer.kt"),
    )
    .expect("IntelliJ stack-trace analyzer must be readable");

    for required in [
        "# Git AI for JetBrains IDEs",
        "## Support status",
        "## Install",
        "## Uninstall",
        "## How attribution works",
        "## Privacy",
        "## Development",
        "## Validation",
        "## License",
        "GitHub Copilot",
        "Junie",
        "github-copilot-jetbrains",
        "notes_backend.kind",
        "git-ai install-hooks",
        "Settings > Plugins",
        "./gradlew buildPlugin",
        "./gradlew check",
        "Apache License 2.0",
    ] {
        assert!(
            readme.contains(required),
            "IntelliJ README is missing plugin-specific fact `{required}`"
        );
    }

    for pattern in [
        "com.github.copilot",
        "com.intellij.ml.llm.matterhorn.junie",
        "com.intellij.ml.llm.matterhorn",
    ] {
        assert!(
            detector.contains(pattern) && readme.contains(pattern),
            "IntelliJ README and detector disagree on package prefix `{pattern}`"
        );
    }

    for stale in [
        "IntelliJ Platform Plugin Template",
        "Use this template",
        "Template Cleanup",
        "Sample code",
        "MyPluginTest",
        "com.github.username.repository",
        "JetBrains Open Source and Community Code of Conduct",
        "github.com/JetBrains/intellij-platform-plugin-template",
    ] {
        assert!(
            ![&readme, &changelog, &conduct]
                .iter()
                .any(|contents| contents.contains(stale)),
            "IntelliJ documentation retains template text `{stale}`"
        );
    }

    assert_eq!(
        readme.matches("<!-- Plugin description -->").count(),
        1,
        "IntelliJ README must retain one Gradle description start marker"
    );
    assert_eq!(
        readme.matches("<!-- Plugin description end -->").count(),
        1,
        "IntelliJ README must retain one Gradle description end marker"
    );
    assert!(
        conduct.contains("[contribution guide](../../CONTRIBUTING.md)"),
        "IntelliJ conduct note must route contributors to the repository guide"
    );

    for required in [
        "# Git AI JetBrains Plugin Changelog",
        "## [Unreleased]",
        "## [0.1.12]",
        "## [0.1.3]",
        "GitHub Copilot and Junie",
    ] {
        assert!(
            changelog.contains(required),
            "IntelliJ changelog is missing project history `{required}`"
        );
    }

    let mut markdown_files = Vec::new();
    collect_markdown_files(&docs_root, &mut markdown_files);
    let mut broken_links = Vec::new();
    for path in markdown_files {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for target in local_markdown_targets(&contents) {
            let target = target.split('#').next().unwrap_or_default();
            if target.is_empty() {
                continue;
            }
            let resolved = path
                .parent()
                .expect("Markdown file must have a parent")
                .join(target);
            if !resolved.exists() {
                broken_links.push(format!(
                    "{} -> {target}",
                    path.strip_prefix(root).unwrap_or(&path).display()
                ));
            }
        }
    }
    assert!(
        broken_links.is_empty(),
        "IntelliJ Markdown contains broken local links:\n{}",
        broken_links.join("\n")
    );
}

#[test]
fn eng_385_design_and_execution_records_have_one_lifecycle_status() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let decisions_root = root.join("docs/decisions");
    let mut documents = Vec::new();
    collect_markdown_files(&decisions_root, &mut documents);
    documents.retain(|path| path.file_name().is_none_or(|name| name != "README.md"));
    documents.extend(
        [
            "docs/pull-rebase-hardening-worklog-2026-06-21.md",
            "docs/pull-rebase-authorship-loss-analysis-2026-06-21.md",
            "docs/session-event-attribution-recovery-plan.md",
            "docs/bash-attribution-recovery-plan.md",
            "specs/metrics-db-timestamps-plan.md",
            "specs/runaway-memory-plan.md",
        ]
        .map(|path| root.join(path)),
    );
    documents.sort();

    let mut violations = Vec::new();
    for path in documents {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let statuses = contents
            .lines()
            .filter_map(|line| line.strip_prefix("Status: "))
            .collect::<Vec<_>>();
        let relative = path.strip_prefix(root).unwrap_or(&path).display();
        if statuses.len() != 1 {
            violations.push(format!("{relative}: found {} status lines", statuses.len()));
            continue;
        }
        if !contents
            .lines()
            .take(6)
            .any(|line| line == format!("Status: {}", statuses[0]))
        {
            violations.push(format!("{relative}: status is not in the document header"));
        }
        if !["accepted", "proposed", "historical", "superseded"]
            .iter()
            .any(|lifecycle| statuses[0].starts_with(lifecycle))
        {
            violations.push(format!(
                "{relative}: unknown lifecycle status `{}`",
                statuses[0]
            ));
        }
        if statuses[0].starts_with("accepted") || statuses[0].starts_with("proposed") {
            for retired in TASK_RETIRED_FRAGMENTS {
                if contents.contains(retired) {
                    violations.push(format!(
                        "{relative}: current record contains retired command `{retired}`"
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "design and execution records need one canonical lifecycle status:\n{}",
        violations.join("\n")
    );

    let index = fs::read_to_string(decisions_root.join("README.md"))
        .expect("decisions index must be readable");
    for required in [
        "`accepted`",
        "`proposed`",
        "`historical`",
        "`superseded`",
        "not current instructions",
        "`task ...` commands are non-operative",
        "../../Makefile",
        "../../CONTRIBUTING.md",
    ] {
        assert!(
            index.contains(required),
            "decisions index is missing lifecycle guidance `{required}`"
        );
    }

    for (relative, lifecycle) in [
        (
            "docs/decisions/2026-07-23-sandbox-checkpoint-continuity-design.md",
            "accepted",
        ),
        (
            "docs/decisions/2026-09-04-keep-line-gutter-default.md",
            "accepted",
        ),
        (
            "docs/decisions/2026-09-04-preserve-unattributed-lines.md",
            "accepted",
        ),
        ("docs/decisions/2026-09-04-reject-lite-mode.md", "accepted"),
        (
            "docs/decisions/2026-09-04-durable-token-usage-design.md",
            "proposed",
        ),
        ("specs/runaway-memory-plan.md", "accepted"),
    ] {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        assert!(
            contents
                .lines()
                .any(|line| line.starts_with(&format!("Status: {lifecycle}"))),
            "{relative} must remain `{lifecycle}`"
        );
    }
}

#[test]
fn eng_386_coverage_docs_match_the_manual_workflow_and_make_targets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let guide =
        fs::read_to_string(root.join("docs/COVERAGE.md")).expect("coverage guide must be readable");
    let workflow = fs::read_to_string(root.join(".github/workflows/coverage.yml"))
        .expect("coverage workflow must be readable");
    let workflow = workflow.replace("\r\n", "\n");
    let makefile = fs::read_to_string(root.join("Makefile")).expect("Makefile must be readable");
    let updater = fs::read_to_string(root.join("scripts/update-coverage-threshold.sh"))
        .expect("coverage threshold updater must be readable");

    for stale in [
        "This threshold is enforced in CI",
        "Pull requests and pushes to main will fail",
        "Coverage reports are always generated",
        "Coverage reports are generated on every CI run",
    ] {
        assert!(
            !guide.contains(stale),
            "coverage guide retains automatic-enforcement claim `{stale}`"
        );
    }

    for required in [
        "manual-only",
        "`workflow_dispatch`",
        "does not run automatically on pull requests or pushes",
        "50% threshold applies only",
        "manual workflow or `make coverage-check`",
        "daemon session timeouts",
        "`llvm-cov` instrumentation",
        "re-enable automatic enforcement",
        "make coverage",
        "make coverage-html",
        "make coverage-lcov",
        "make coverage-check",
        "COVERAGE_THRESHOLD=",
        "cargo-llvm-cov",
    ] {
        assert!(
            guide.contains(required),
            "coverage guide is missing current behavior `{required}`"
        );
    }

    assert!(
        workflow.contains("on:\n  workflow_dispatch:")
            && !workflow.contains("pull_request:")
            && !workflow.contains("push:"),
        "coverage workflow must remain manual-only while the guide says it is"
    );
    for required in [
        "COVERAGE_THRESHOLD: 50",
        "--fail-under-lines $COVERAGE_THRESHOLD",
        "if: always()",
        "retention-days: 30",
    ] {
        assert!(
            workflow.contains(required),
            "coverage workflow is missing documented behavior `{required}`"
        );
    }
    for required in [
        "COVERAGE_THRESHOLD ?= 50",
        "coverage:\n\tcargo llvm-cov test",
        "coverage-html:\n\tcargo llvm-cov test",
        "coverage-lcov:\n\tcargo llvm-cov test",
        "coverage-check:\n\tcargo llvm-cov test",
        "--fail-under-lines $(COVERAGE_THRESHOLD)",
    ] {
        assert!(
            makefile.contains(required),
            "Makefile is missing documented coverage behavior `{required}`"
        );
    }
    for required in [
        ".github/workflows/coverage.yml",
        "^COVERAGE_THRESHOLD ?=",
        "Makefile.bak",
    ] {
        assert!(
            updater.contains(required),
            "coverage updater does not keep both defaults aligned: missing `{required}`"
        );
    }
}

#[test]
fn eng_387_repository_declares_one_rust_minimum() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cargo_toml =
        fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml must be readable");
    let agents = fs::read_to_string(root.join("AGENTS.md")).expect("AGENTS.md must be readable");
    let readme = fs::read_to_string(root.join("README.md")).expect("README.md must be readable");
    let contributing =
        fs::read_to_string(root.join("CONTRIBUTING.md")).expect("CONTRIBUTING.md must be readable");
    let nix_readme =
        fs::read_to_string(root.join("README-nix.md")).expect("README-nix.md must be readable");
    let flake = fs::read_to_string(root.join("flake.nix")).expect("flake.nix must be readable");

    assert!(
        cargo_toml.contains("rust-version = \"1.93\""),
        "Cargo.toml must declare Rust 1.93 as the package MSRV"
    );
    for (relative, contents) in [
        ("AGENTS.md", agents.as_str()),
        ("README.md", readme.as_str()),
        ("CONTRIBUTING.md", contributing.as_str()),
        ("README-nix.md", nix_readme.as_str()),
    ] {
        assert!(
            contents.contains("Rust 1.93.0 or newer"),
            "{relative} must state the repository MSRV"
        );
        assert!(
            !contents.contains("Rust 1.97"),
            "{relative} still claims a different Rust minimum"
        );
    }
    for required in [
        "minimum supported Rust 1.93.0",
        "pkgs.rust-bin.stable.\"1.93.0\"",
        "MSRV toolchain",
    ] {
        assert!(
            flake.contains(required),
            "flake.nix is missing the Rust minimum contract `{required}`"
        );
    }

    for relative in [
        ".github/workflows/coverage.yml",
        ".github/workflows/lint-format.yml",
        ".github/workflows/nightly-agent-integration.yml",
        ".github/workflows/release.yml",
    ] {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        assert!(
            contents.contains("1.93.0"),
            "{relative} must exercise the repository MSRV"
        );
        assert!(
            !contents.contains("toolchain: \"1.97") && !contents.contains("default-toolchain 1.97"),
            "{relative} pins a Rust version above the repository MSRV"
        );
    }

    let stable_marker = "# Intentionally tracks current stable above the 1.93.0 MSRV.";
    let mut workflow_files = Vec::new();
    collect_files(&root.join(".github"), &mut workflow_files);
    for path in workflow_files.into_iter().filter(|path| {
        matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yml" | "yaml")
        )
    }) {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let lines = contents.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            if line.trim() == "toolchain: stable" {
                assert!(
                    index > 0 && lines[index - 1].trim() == stable_marker,
                    "{}:{} tracks stable Rust without stating that it is intentionally above the MSRV",
                    path.strip_prefix(root).unwrap_or(&path).display(),
                    index + 1
                );
            }
        }
    }

    let benchmark_setup =
        fs::read_to_string(root.join(".github/actions/setup-performance-benchmarks/action.yml"))
            .expect("performance benchmark setup action must be readable");
    assert!(
        !benchmark_setup.contains("pinned Rust/Python"),
        "performance setup must not describe a stable-tracking Rust toolchain as pinned"
    );
}

#[test]
fn eng_388_readme_qualifies_the_no_heuristics_claim() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).expect("README.md must be readable");
    let vscode = fs::read_to_string(root.join("agent-support/vscode/README.md"))
        .expect("VS Code README must be readable");
    let visual_studio = fs::read_to_string(root.join("agent-support/visualstudio/DESIGN.md"))
        .expect("Visual Studio design must be readable");
    let intellij = fs::read_to_string(root.join("agent-support/intellij/README.md"))
        .expect("IntelliJ README must be readable");

    assert!(
        !readme.contains("does not\nuse heuristics or an AI detector")
            && !readme.contains("does not use heuristics or an AI detector"),
        "README must not make an absolute no-heuristics claim"
    );
    for required in [
        "editor integrations may recognize agent-originated edits",
        "high-confidence event or call-stack signatures",
        "does not inspect source content with an AI detector",
        "Unknown edits remain unknown or untracked",
    ] {
        assert!(
            readme.contains(required),
            "README is missing the qualified evidence boundary `{required}`"
        );
    }
    assert!(
        readme.lines().count() <= 125,
        "README introduction update must preserve the concise README line budget"
    );

    for (relative, contents, required) in [
        (
            "agent-support/vscode/README.md",
            vscode.as_str(),
            "effectiveness of the heuristics",
        ),
        (
            "agent-support/visualstudio/DESIGN.md",
            visual_studio.as_str(),
            "URI scheme sniffing",
        ),
        (
            "agent-support/intellij/README.md",
            intellij.as_str(),
            "high-confidence stack-trace package prefixes",
        ),
    ] {
        assert!(
            contents.contains(required),
            "{relative} no longer documents its editor-event recognition mechanism `{required}`"
        );
    }
    assert!(
        visual_studio.contains("must remain\nunattributed")
            && intellij.contains("ambiguous matches are not labeled as AI"),
        "editor integration docs must keep ambiguous edits unattributed"
    );
}

#[test]
fn eng_391_vscode_cursor_readme_matches_installer_lifecycle() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/vscode/README.md"))
        .expect("VS Code README must be readable");
    let package = fs::read_to_string(root.join("agent-support/vscode/package.json"))
        .expect("VS Code package metadata must be readable");
    let constants = fs::read_to_string(root.join("agent-support/vscode/src/consts.ts"))
        .expect("VS Code constants must be readable");
    let vscode = fs::read_to_string(root.join("src/operations/mdm/agents/vscode.rs"))
        .expect("VS Code installer must be readable");
    let cursor = fs::read_to_string(root.join("src/operations/mdm/agents/cursor.rs"))
        .expect("Cursor installer must be readable");

    for required in [
        "## Support status",
        "VS Code 1.99.3 or newer",
        "Cursor 1.7 or newer",
        "git-ai 1.0.23 or newer",
        "git-ai.git-ai-vscode",
        "externally operated Marketplace",
        "~/.cursor/hooks.json",
        "chat.useHooks",
        "github.copilot.chat.otel.dbSpanExporter.enabled",
        "git-ai uninstall-hooks --dry-run=false",
        "does not remove the extension",
        "restart Cursor",
        "../../data-privacy.md",
    ] {
        assert!(
            readme.contains(required),
            "VS Code/Cursor README is missing lifecycle fact `{required}`"
        );
    }

    for stale in ["Restart VS Code", "latest release of the `git-ai` CLI"] {
        assert!(
            !readme.contains(stale),
            "VS Code/Cursor README retains stale instruction `{stale}`"
        );
    }

    for source_fact in [
        (package.as_str(), "\"vscode\": \">=1.99.3\""),
        (constants.as_str(), "MIN_GIT_AI_VERSION = \"1.0.23\""),
        (vscode.as_str(), "GIT_AI_VSCODE_EXTENSION_ID"),
        (vscode.as_str(), "update_vscode_chat_hook_settings"),
        (cursor.as_str(), "MIN_CURSOR_VERSION"),
        (cursor.as_str(), "hooks.json"),
    ] {
        assert!(
            source_fact.0.contains(source_fact.1),
            "installer/package source is missing documented fact `{}`",
            source_fact.1
        );
    }
}

#[test]
fn eng_392_privacy_docs_disclose_editor_telemetry_gate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let privacy =
        fs::read_to_string(root.join("data-privacy.md")).expect("privacy guide must be readable");
    let vscode = fs::read_to_string(root.join("agent-support/vscode/README.md"))
        .expect("VS Code README must be readable");
    let vscode_source = fs::read_to_string(root.join("agent-support/vscode/src/extension.ts"))
        .expect("VS Code extension source must be readable");
    let intellij = fs::read_to_string(root.join("agent-support/intellij/README.md"))
        .expect("IntelliJ README must be readable");
    let intellij_source = fs::read_to_string(root.join(
        "agent-support/intellij/src/main/kotlin/org/jetbrains/plugins/template/services/TelemetryService.kt",
    ))
    .expect("IntelliJ telemetry source must be readable");

    for required in [
        "CLI and daemon telemetry is off by default",
        "Bundled editor extension exception",
        "legacy `telemetry_oss` gate",
        "missing setting is not an opt-out",
        "https://us.i.posthog.com",
        "ingest.us.sentry.io",
        "externally operated",
    ] {
        assert!(
            privacy.contains(required),
            "privacy guide is missing editor telemetry boundary `{required}`"
        );
    }
    for required in [
        "## Telemetry",
        "vscode_extension_startup",
        "telemetry_oss",
        "https://us.i.posthog.com",
        "missing setting",
    ] {
        assert!(
            vscode.contains(required),
            "VS Code README is missing telemetry fact `{required}`"
        );
    }
    assert!(
        intellij.contains("missing setting is not treated as an")
            && intellij.contains("opt-out by the plugin")
            && intellij.contains("PostHog analytics and Sentry error reporting"),
        "IntelliJ README must retain its legacy telemetry gate"
    );
    for (source, fact) in [
        (vscode_source.as_str(), "config.telemetry_oss === \"off\""),
        (vscode_source.as_str(), "https://us.i.posthog.com"),
        (intellij_source.as_str(), "telemetry_oss"),
        (intellij_source.as_str(), "ingest.us.sentry.io"),
    ] {
        assert!(
            source.contains(fact),
            "editor source is missing fact `{fact}`"
        );
    }
}

#[test]
fn eng_393_nix_development_uses_gnu_make_interface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let flake = fs::read_to_string(root.join("flake.nix")).expect("flake.nix must be readable");
    let readme =
        fs::read_to_string(root.join("README-nix.md")).expect("Nix README must be readable");

    assert!(
        flake.contains("gnumake") && flake.contains("Run 'make build'"),
        "Nix development shell must provide and recommend GNU Make"
    );
    assert!(
        !flake.contains("Run 'cargo build'"),
        "Nix shell messages must not bypass the canonical Make interface"
    );
    for required in ["make build", "make test", "git-ai --version"] {
        assert!(
            readme.contains(required),
            "Nix development guide is missing canonical command `{required}`"
        );
    }
    for bypass in ["cargo build\ncargo test", "cargo run -- --version"] {
        assert!(
            !readme.contains(bypass),
            "Nix development guide retains direct Cargo workflow `{bypass}`"
        );
    }
}

#[test]
fn eng_394_nix_readme_has_owner_aware_uninstall_sequence() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme =
        fs::read_to_string(root.join("README-nix.md")).expect("Nix README must be readable");

    for required in [
        "## Uninstall",
        "git-ai uninstall-hooks --dry-run=false",
        "nix profile list",
        "nix profile remove git-ai",
        "home-manager switch",
        "nixos-rebuild switch",
        "darwin-rebuild switch",
        "cannot remove a Nix-owned package or declaration",
        "git-ai uninstall --yes --purge",
        "repo-local `.git/ai`",
        "nix3-profile-remove",
    ] {
        assert!(
            readme.contains(required),
            "Nix README is missing uninstall boundary `{required}`"
        );
    }
}

#[test]
fn eng_395_nix_wrapper_selection_uses_package_outputs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let flake = fs::read_to_string(root.join("flake.nix")).expect("flake.nix must be readable");
    let readme =
        fs::read_to_string(root.join("README-nix.md")).expect("Nix README must be readable");

    assert!(
        !flake.contains("setGitAlias") && !readme.contains("setGitAlias"),
        "Nix must not expose an inert wrapper-selection option"
    );
    for required in [
        "packages.${system}.default",
        "packages.${system}.minimal",
        "package = git-ai.packages.x86_64-linux.minimal;",
        "latest`, `next`, `enterprise-latest`, or `enterprise-next",
    ] {
        assert!(
            readme.contains(required),
            "Nix README is missing executable option fact `{required}`"
        );
    }
    assert!(
        !readme.contains("environment.systemPackages = ["),
        "NixOS module example must not install the module package twice"
    );
    assert!(
        flake.contains("default = git-ai-package;") && flake.contains("minimal = git-ai-minimal;"),
        "flake must retain explicit full and minimal wrapper package outputs"
    );
}

#[test]
fn eng_396_remaining_historical_records_are_classified() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (relative, lifecycle, authority) in [
        (
            "docs/pr-1153-review-findings.md",
            "historical",
            "docs/contracts/",
        ),
        (
            "docs/rewrite-simplification-spec.md",
            "superseded",
            "docs/architecture/rewrite-ops-spec.md",
        ),
        (
            "docs/migrations/sessions-v2-note-format.md",
            "historical",
            "specs/git_ai_standard_v3.0.0.md",
        ),
    ] {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        let statuses = contents
            .lines()
            .filter(|line| line.starts_with("Status: "))
            .collect::<Vec<_>>();
        assert_eq!(
            statuses.len(),
            1,
            "{relative} must have one lifecycle status"
        );
        assert!(
            contents
                .lines()
                .take(6)
                .any(|line| line.starts_with(&format!("Status: {lifecycle}"))),
            "{relative} must be classified as {lifecycle} in its header"
        );
        assert!(
            contents.contains(authority),
            "{relative} must name current authority `{authority}`"
        );
    }
}

#[test]
fn eng_397_cli_output_contract_names_current_json_sources() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let contract = fs::read_to_string(root.join("docs/contracts/cli-output.md"))
        .expect("CLI output contract must be readable");
    let source_paths = [
        "src/model/diff_json.rs",
        "src/operations/commands/blame/json_output.rs",
        "src/operations/commands/blame/porcelain.rs",
        "src/operations/commands/status.rs",
        "src/operations/commands/usage.rs",
        "src/operations/commands/fetch_notes.rs",
    ];

    for source_path in source_paths {
        assert!(
            root.join(source_path).is_file(),
            "contract source path must resolve: {source_path}"
        );
        assert!(
            contract.contains(source_path),
            "CLI output contract omits source path `{source_path}`"
        );
    }
    for required in [
        "`git-ai blame --json <file>`",
        "`lines`",
        "`prompts`",
        "`metadata`",
        "`other_files`",
        "`commits`",
        "Blame porcelain",
    ] {
        assert!(
            contract.contains(required),
            "CLI output contract is missing blame JSON fact `{required}`"
        );
    }
    assert!(
        !contract.contains(".rs:"),
        "live CLI contract must not use brittle Rust source line numbers"
    );
}

#[test]
fn eng_399_opencode_docs_match_managed_plugin_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/opencode/README.md"))
        .expect("OpenCode README must be readable");
    let plugin = fs::read_to_string(root.join("agent-support/opencode/git-ai.ts"))
        .expect("OpenCode plugin must be readable");
    let installer = fs::read_to_string(root.join("src/operations/mdm/agents/opencode.rs"))
        .expect("OpenCode installer must be readable");

    for required in [
        "## Support status",
        "`opencode` and `opencode2`",
        "~/.config/opencode/plugins/git-ai.ts",
        "~/.config/opencode/plugin/git-ai.ts",
        "git-ai uninstall-hooks --dry-run=false",
        "make build",
        "10-second",
        "tool input",
        "session ID",
        "local git-ai CLI",
        "does not make direct network requests",
        "../../data-privacy.md",
        "Apache License 2.0",
    ] {
        assert!(
            readme.contains(required),
            "OpenCode README is missing integration fact `{required}`"
        );
    }
    for stale in ["`cargo build`", "`cargo run -- install-hooks`"] {
        assert!(
            !readme.contains(stale),
            "OpenCode README retains unsupported development command `{stale}`"
        );
    }
    assert!(
        plugin.contains("untracked or AI-authored")
            && !plugin.contains("mark code changes as human or AI-authored"),
        "OpenCode plugin header must not turn the compatibility boundary into human evidence"
    );
    for source_fact in [
        "detect_binary_names: &[\"opencode\", \"opencode2\"]",
        ".join(\"plugins\")",
        ".join(\"plugin\")",
    ] {
        assert!(
            installer.contains(source_fact),
            "OpenCode installer is missing documented fact `{source_fact}`"
        );
    }
}

#[test]
fn eng_400_pi_docs_match_managed_extension_contract() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("agent-support/pi/README.md"))
        .expect("Pi README must be readable");
    let extension = fs::read_to_string(root.join("agent-support/pi/git-ai.ts"))
        .expect("Pi extension must be readable");
    let installer = fs::read_to_string(root.join("src/operations/mdm/agents/pi.rs"))
        .expect("Pi installer must be readable");

    for required in [
        "## Support status",
        "`pi`",
        "~/.pi/agent/extensions/git-ai.ts",
        "~/.pi/agent/git-ai.override.json",
        "git-ai uninstall-hooks --dry-run=false",
        "user-owned",
        "left in place",
        "session path",
        "session ID",
        "model",
        "tool input",
        "tool result",
        "dirty file contents",
        "Bash commands",
        "before and after",
        "local `git-ai` CLI",
        "does not make direct network requests",
        "../../data-privacy.md",
        "Apache License 2.0",
    ] {
        assert!(
            readme.contains(required),
            "Pi README is missing integration fact `{required}`"
        );
    }
    for source_fact in [
        "detect_binary_names: &[\"pi\"]",
        ".join(\"extensions\")",
        ".join(\"git-ai.ts\")",
    ] {
        assert!(
            installer.contains(source_fact),
            "Pi installer is missing documented fact `{source_fact}`"
        );
    }
    for source_fact in [
        "git-ai.override.json",
        "hook_event_name: 'before_command'",
        "hook_event_name: 'after_command'",
        "dirty_files: await readDirtyFiles(call.filepaths)",
        "tool_input: call.toolInput",
        "tool_result: {",
    ] {
        assert!(
            extension.contains(source_fact),
            "Pi extension is missing documented fact `{source_fact}`"
        );
    }
}

#[test]
fn eng_401_live_architecture_docs_use_stable_source_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inventory = fs::read_to_string(root.join("docs/architecture/inventory.md"))
        .expect("architecture inventory must be readable");
    let ownership = fs::read_to_string(root.join("docs/architecture/state-ownership.md"))
        .expect("state ownership map must be readable");

    assert!(
        ownership.contains("Verified 2026-09-06"),
        "state ownership verification date must reflect this source review"
    );
    for (relative, contents) in [
        ("docs/architecture/inventory.md", &inventory),
        ("docs/architecture/state-ownership.md", &ownership),
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
