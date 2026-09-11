use super::*;

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
fn eng_401_live_architecture_docs_use_stable_source_references() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inventory = fs::read_to_string(root.join("docs/architecture/inventory.md"))
        .expect("architecture inventory must be readable");
    let ownership = fs::read_to_string(root.join("docs/architecture/state-ownership.md"))
        .expect("state ownership map must be readable");

    assert_live_architecture_references(root, &inventory, &ownership);
}

#[test]
fn eng_409_architecture_contract_allows_review_wording_and_date_updates() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inventory = fs::read_to_string(root.join("docs/architecture/inventory.md")).unwrap();
    let ownership = fs::read_to_string(root.join("docs/architecture/state-ownership.md")).unwrap();
    let revised = regex::Regex::new(r"Verified \d{4}-\d{2}-\d{2}")
        .unwrap()
        .replace_all(&ownership, "Source checked 2030-01-02")
        .replace('\n', "\r\n");
    assert_ne!(revised, ownership);
    assert_live_architecture_references(root, &inventory, &revised);
    let broken = format!("{revised}\nsrc/config/mod.rs:123");
    assert!(
        std::panic::catch_unwind(|| {
            assert_live_architecture_references(root, &inventory, &broken);
        })
        .is_err(),
        "line-offset citations must still fail"
    );
}
