use super::{Path, coverage_defaults_agree, fs};

#[test]
fn eng_386_coverage_docs_match_the_manual_workflow_and_make_targets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let guide =
        fs::read_to_string(root.join("docs/COVERAGE.md")).expect("coverage guide must be readable");
    let workflow = fs::read_to_string(root.join(".github/workflows/coverage.yml"))
        .expect("coverage workflow must be readable");
    let workflow = workflow.replace("\r\n", "\n");
    let makefile = fs::read_to_string(root.join("Makefile")).expect("Makefile must be readable");
    let makefile = makefile.replace("\r\n", "\n");
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
        "../Makefile",
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

    let triggers = workflow
        .lines()
        .skip_while(|line| line.trim() != "on:")
        .skip(1)
        .take_while(|line| line.trim().is_empty() || line.starts_with(char::is_whitespace))
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    assert_eq!(
        triggers,
        ["workflow_dispatch:"],
        "coverage is documented as manual-only"
    );
    assert!(coverage_defaults_agree(&makefile, &workflow));
    for required in ["--fail-under-lines $COVERAGE_THRESHOLD", "if: always()"] {
        assert!(
            workflow.contains(required),
            "coverage workflow is missing documented behavior `{required}`"
        );
    }
    for required in [
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
fn eng_409_coverage_defaults_allow_coordinated_changes_but_reject_drift() {
    let makefile = "# Updated baseline\nCOVERAGE_THRESHOLD ?= 65\n";
    let workflow = "env:\r\n  # Independently worded comment\r\n  COVERAGE_THRESHOLD: 65\r\n";
    assert!(coverage_defaults_agree(makefile, workflow));
    assert!(!coverage_defaults_agree(
        makefile,
        &workflow.replace(": 65", ": 66")
    ));
    assert!(!coverage_defaults_agree(
        makefile,
        "# COVERAGE_THRESHOLD: 65"
    ));
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

    let manifest: toml::Value = toml::from_str(&cargo_toml).unwrap();
    let declared = manifest["package"]["rust-version"].as_str().unwrap();
    let msrv = if declared.split('.').count() == 2 {
        format!("{declared}.0")
    } else {
        declared.to_string()
    };
    let documented_rust = regex::Regex::new(r"Rust\s+`?(?:>=\s*)?(\d+\.\d+(?:\.\d+)?)").unwrap();
    for (relative, contents) in [
        ("AGENTS.md", agents.as_str()),
        ("README.md", readme.as_str()),
        ("CONTRIBUTING.md", contributing.as_str()),
        ("README-nix.md", nix_readme.as_str()),
    ] {
        let versions = documented_rust
            .captures_iter(contents)
            .map(|capture| capture[1].to_string())
            .collect::<Vec<_>>();
        assert!(
            !versions.is_empty(),
            "{relative} must document the Rust minimum"
        );
        assert!(
            versions
                .iter()
                .all(|version| version == &msrv || version == declared),
            "{relative} Rust declarations {versions:?} disagree with Cargo.toml ({declared})"
        );
    }
    assert!(
        flake.contains(&format!("pkgs.rust-bin.stable.\"{msrv}\"")),
        "Nix toolchain must use the Cargo.toml minimum"
    );

    // Windows executable search prefers the WSL stub unless PATH is resolved first.
    let bash_name = if cfg!(windows) { "bash.exe" } else { "bash" };
    let bash_path = std::env::split_paths(&std::env::var_os("PATH").expect("PATH must be set"))
        .map(|directory| directory.join(bash_name))
        .find(|candidate| candidate.is_file())
        .expect("Git Bash must be available on PATH");
    let resolved = std::process::Command::new(bash_path)
        .arg(".github/actions/setup-rust/resolve.sh")
        .arg("msrv")
        .current_dir(root)
        .output()
        .expect("Rust minimum resolver must run");
    assert!(resolved.status.success(), "{:?}", resolved);
    assert_eq!(String::from_utf8(resolved.stdout).unwrap().trim(), msrv);

    let pinned_toolchain =
        regex::Regex::new(r#"(?:toolchain:\s*["']?|--default-toolchain\s+)(\d+\.\d+\.\d+)"#)
            .unwrap();
    for relative in [
        ".github/workflows/coverage.yml",
        ".github/workflows/lint-format.yml",
        ".github/workflows/nightly-agent-integration.yml",
        ".github/workflows/release.yml",
    ] {
        let contents = fs::read_to_string(root.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        let executable = contents
            .lines()
            .filter(|line| !line.trim().starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            executable
                .lines()
                .any(|line| line.trim() == "toolchain: msrv"),
            "{relative} must exercise the repository MSRV"
        );
        assert!(
            !pinned_toolchain.is_match(&executable),
            "{relative} must resolve the MSRV from Cargo.toml instead of duplicating it"
        );
    }

    let release = fs::read_to_string(root.join(".github/workflows/release.yml")).unwrap();
    assert!(release.contains("bash .github/actions/setup-rust/resolve.sh msrv"));
    assert!(release.contains("--default-toolchain $RUST_TOOLCHAIN --target"));

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
    let visual_studio = visual_studio.replace("\r\n", "\n");
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
