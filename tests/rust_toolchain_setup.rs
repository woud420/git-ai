use std::{fs, path::Path};

const SETUP: &str = "uses: ./.github/actions/setup-rust";

#[test]
fn eng_411_callers_preserve_toolchain_selection() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (relative, expected) in [
        (
            "actions/setup-performance-benchmarks/action.yml",
            vec!["stable"],
        ),
        ("workflows/coverage.yml", vec!["msrv"]),
        ("workflows/e2e-tests.yml", vec!["stable"]),
        ("workflows/git-core-compat.yml", vec!["stable"]),
        ("workflows/github-integration-tests.yml", vec!["stable"]),
        (
            "workflows/install-scripts-local.yml",
            vec!["stable", "stable"],
        ),
        ("workflows/lint-format.yml", vec!["msrv", "msrv", "msrv"]),
        (
            "workflows/nightly-agent-integration.yml",
            vec!["msrv", "msrv"],
        ),
        ("workflows/release.yml", vec!["msrv", "msrv", "stable"]),
        ("workflows/test.yml", vec!["stable", "stable"]),
    ] {
        let contents = fs::read_to_string(root.join(".github").join(relative)).unwrap();
        assert!(
            !contents.contains("uses: dtolnay/rust-toolchain@"),
            "{relative}"
        );
        assert_eq!(
            contents.matches(SETUP).count(),
            expected.len(),
            "{relative}"
        );
        let actual: Vec<_> = contents
            .lines()
            .filter_map(|line| line.trim().strip_prefix("toolchain:").map(str::trim))
            .collect();
        assert_eq!(actual, expected, "{relative}");
    }
}

#[test]
fn eng_411_setup_forwards_inputs_and_owns_the_installer_pin() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let action = fs::read_to_string(root.join(".github/actions/setup-rust/action.yml")).unwrap();
    let pin = action
        .lines()
        .find_map(|line| line.trim().strip_prefix("- uses: dtolnay/rust-toolchain@"))
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap();
    assert_eq!(pin.len(), 40);
    assert!(pin.bytes().all(|byte| byte.is_ascii_hexdigit()));
    for mapping in [
        "TOOLCHAIN_POLICY: ${{ inputs.toolchain }}",
        "toolchain: ${{ steps.resolve.outputs.toolchain }}",
        "components: ${{ inputs.components }}",
        "targets: ${{ inputs.targets }}",
    ] {
        assert!(action.contains(mapping), "missing {mapping}");
    }
}

#[test]
fn eng_411_filtered_workflows_watch_shared_setup() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (workflow, count) in [
        ("lint-format", 2),
        ("test", 2),
        ("git-core-compat", 2),
        ("install-scripts-local", 1),
    ] {
        let contents =
            fs::read_to_string(root.join(format!(".github/workflows/{workflow}.yml"))).unwrap();
        assert_eq!(
            contents.matches("'.github/actions/setup-rust/**'").count(),
            count,
            "{workflow}"
        );
    }
    let benchmarks =
        fs::read_to_string(root.join(".github/workflows/performance-benchmarks.yml")).unwrap();
    assert!(benchmarks.contains("'.github/actions/**'"));
}
