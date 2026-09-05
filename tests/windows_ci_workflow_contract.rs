#[cfg(windows)]
use std::process::Command;

const TEST_WORKFLOW: &str = include_str!("../.github/workflows/test.yml");
const LINT_WORKFLOW: &str = include_str!("../.github/workflows/lint-format.yml");
const WINDOWS_STEP_NAME: &str = "      - name: Run tests (Windows)";
const RUN_BLOCK: &str = "        run: |";
const SCRIPT_INDENT: &str = "          ";
const NATIVE_FAILURE_GUARD: &str = "if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }";
const MAKE_INSTALL_COMMAND: &str = "choco install make --no-progress --yes";

const WINDOWS_TEST_COMMANDS: [&str; 3] = [
    r#"make test CARGO_TEST_ARGS="$($testTargets -join ' ')" TEST_THREADS=${{ matrix.test_threads }}"#,
    r#"make test CARGO_TEST_ARGS="--doc" TEST_THREADS=${{ matrix.test_threads }}"#,
    r#"make test CARGO_TEST_ARGS="--test integration" EXTRA_TEST_BINARY_ARGS="$skipArgs" TEST_THREADS=${{ matrix.test_threads }}"#,
];

fn windows_test_script() -> Vec<&'static str> {
    let step = TEST_WORKFLOW
        .split_once(WINDOWS_STEP_NAME)
        .expect("Windows test step must exist")
        .1;
    let script = step
        .split_once(RUN_BLOCK)
        .expect("Windows test step must have a script body")
        .1;

    script
        .lines()
        .skip_while(|line| line.is_empty())
        .take_while(|line| line.is_empty() || line.starts_with(SCRIPT_INDENT))
        .map(str::trim)
        .collect()
}

#[test]
fn windows_test_commands_propagate_native_failures_immediately() {
    let script = windows_test_script();
    let actual_commands: Vec<_> = script
        .iter()
        .copied()
        .filter(|line| line.starts_with("make test "))
        .collect();
    assert_eq!(actual_commands, WINDOWS_TEST_COMMANDS);

    for command in WINDOWS_TEST_COMMANDS {
        let index = script
            .iter()
            .position(|line| *line == command)
            .expect("Windows test command must exist");
        assert_eq!(
            script.get(index + 1).copied(),
            Some(NATIVE_FAILURE_GUARD),
            "missing native-exit guard after `{command}`"
        );
    }

    let doctest_index = script
        .iter()
        .position(|line| *line == WINDOWS_TEST_COMMANDS[1])
        .expect("Windows doctest command must exist");
    assert_eq!(script.get(doctest_index + 2).copied(), Some("exit 0"));
}

#[test]
fn windows_ci_provisions_gnu_make_and_checks_the_installer_exit_code() {
    for (name, workflow) in [("test", TEST_WORKFLOW), ("lint", LINT_WORKFLOW)] {
        let lines = workflow.lines().map(str::trim).collect::<Vec<_>>();
        let install_index = lines
            .iter()
            .position(|line| *line == MAKE_INSTALL_COMMAND)
            .unwrap_or_else(|| panic!("{name} workflow must explicitly install GNU Make"));
        assert_eq!(
            lines.get(install_index + 1).copied(),
            Some(NATIVE_FAILURE_GUARD),
            "{name} workflow must propagate the GNU Make installer exit code"
        );
    }
}

#[cfg(windows)]
#[test]
fn last_exit_code_guard_preserves_the_native_failure_code() {
    let output = Command::new("pwsh")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "cmd /c exit 7; if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; cmd /c exit 0",
        ])
        .output()
        .expect("pwsh must be available on Windows test runners");

    assert_eq!(
        output.status.code(),
        Some(7),
        "a later success masked native exit 7\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
