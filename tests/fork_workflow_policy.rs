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
        intellij.contains("LICENSE                 License, Apache-2.0"),
        "IntelliJ project tree must describe its bundled Apache-2.0 license"
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
