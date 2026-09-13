use super::{Path, fs};

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
