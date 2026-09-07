#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn eng_410_unix_setup_accepts_the_supported_make_range() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let action_path = root.join(".github/actions/setup-gnu-make");
    let action = fs::read_to_string(action_path.join("action.yml")).unwrap();
    let step = action
        .split("- name: Verify GNU Make (Unix)")
        .nth(1)
        .unwrap()
        .split("- name: Verify GNU Make (Windows)")
        .next()
        .unwrap()
        .split("run: |")
        .nth(1)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.strip_prefix("        ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let real_make = ["gmake", "make"]
        .into_iter()
        .find(|candidate| {
            Command::new(candidate)
                .arg("--version")
                .output()
                .is_ok_and(|output| {
                    output.status.success()
                        && String::from_utf8_lossy(&output.stdout).starts_with("GNU Make ")
                })
                && Command::new(candidate)
                    .args([
                        "--no-print-directory",
                        "--dry-run",
                        "build",
                        "TEST_THREADS=1",
                    ])
                    .current_dir(root)
                    .output()
                    .is_ok_and(|output| output.status.success())
        })
        .expect("the repository requires GNU Make");

    // Exercise the actual action script and Makefile guard with simulated release
    // versions; no package installation or build recipe is executed.
    let script = format!(
        r#"
make() {{
  if [ "$1" = "--version" ]; then
    printf '%s\n' "$TEST_MAKE_BANNER"
  else
    command "$TEST_REAL_MAKE" "$@" "MAKE_VERSION=$TEST_MAKE_VERSION"
  fi
}}
{step}
"#
    );
    for (version, supported) in [
        ("4.4.1", true),
        ("4.4.2", true),
        ("4.5", true),
        ("5.0", true),
        ("4.4.0", false),
        ("4.3", false),
        ("3.81", false),
        ("BSD", false),
    ] {
        let output = Command::new("bash")
            .args(["-c", &script])
            .current_dir(root)
            .env("GITHUB_ACTION_PATH", &action_path)
            .env("TEST_REAL_MAKE", real_make)
            .env("TEST_MAKE_VERSION", version)
            .env(
                "TEST_MAKE_BANNER",
                if version == "BSD" {
                    "BSD make".to_string()
                } else {
                    format!("GNU Make {version}")
                },
            )
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            supported,
            "{version}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
