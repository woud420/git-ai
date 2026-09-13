use super::{ACTIVE_DISTRIBUTION_FILES, Path, STALE_DISTRIBUTION_FRAGMENTS, fs};

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
        "git-ai uninstall --yes",
        "nix profile list",
        "nix profile remove git-ai",
        "home-manager switch",
        "nixos-rebuild switch",
        "darwin-rebuild switch",
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
