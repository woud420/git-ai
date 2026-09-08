#![cfg(unix)]

use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const HELPERS_START: &str = "# BEGIN WSL PACKAGE INSTALLER HELPERS";
const HELPERS_END: &str = "# END WSL PACKAGE INSTALLER HELPERS";

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn extracted_helpers(repo: &TestRepo) -> PathBuf {
    let installer_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh");
    let installer = fs::read_to_string(installer_path).unwrap();
    let start = installer
        .find(HELPERS_START)
        .expect("install.sh must expose the WSL package helper boundary")
        + HELPERS_START.len();
    let end = installer[start..]
        .find(HELPERS_END)
        .map(|offset| start + offset)
        .expect("install.sh must close the WSL package helper boundary");
    let helper_path = repo.path().join("wsl-package-helpers.sh");
    let prelude = r#"set -euo pipefail
REPO="woud420/git-ai"
EMBEDDED_CHECKSUMS="__CHECKSUMS_PLACEHOLDER__"
API_BASE="https://enterprise.example"
API_KEY="test-secret-value"
warn() { printf 'Warning: %s\n' "$1" >&2; }
success() { printf '%s\n' "$1"; }
verify_checksum() { :; }
"#;
    fs::write(
        &helper_path,
        format!("{prelude}\n{}", &installer[start..end]),
    )
    .unwrap();
    helper_path
}

struct HostFixture {
    repo: TestRepo,
    fake_bin: PathBuf,
    windows_home: PathBuf,
    local_msi: PathBuf,
    cmd_log: PathBuf,
    wslpath_log: PathBuf,
    msiexec_log: PathBuf,
}

impl HostFixture {
    fn new() -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let fake_bin = repo.path().join("fake-bin");
        let windows_home = repo.path().join("windows-user");
        let local_msi = repo.path().join("git-ai-windows-x64.msi");
        let cmd_log = repo.path().join("cmd.log");
        let wslpath_log = repo.path().join("wslpath.log");
        let msiexec_log = repo.path().join("msiexec.log");
        fs::create_dir_all(&fake_bin).unwrap();
        fs::create_dir_all(&windows_home).unwrap();
        fs::write(&local_msi, b"test msi").unwrap();

        write_executable(
            &fake_bin.join("cmd.exe"),
            r#"#!/bin/sh
printf 'cmd\n' >> "$TEST_CMD_LOG"
printf 'C:\Users\Test User\r\n'
"#,
        );
        write_executable(
            &fake_bin.join("wslpath"),
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$TEST_WSLPATH_LOG"
case "$1" in
    -u) printf '%s\n' "$TEST_WINDOWS_HOME" ;;
    -w) printf 'C:\Users\Test User\AppData\Local\Temp\git-ai-windows-x64.msi\r\n' ;;
    *) exit 2 ;;
esac
"#,
        );
        write_executable(
            &fake_bin.join("msiexec.exe"),
            r#"#!/bin/sh
printf '%s\n' '--call--' "$@" >> "$TEST_MSIEXEC_LOG"
if [ -n "${TEST_INSTALLED_DIR:-}" ]; then
    /bin/mkdir -p "$TEST_INSTALLED_DIR"
    : > "$TEST_INSTALLED_DIR/git-ai.exe"
fi
exit "${TEST_MSIEXEC_EXIT:-0}"
"#,
        );

        Self {
            repo,
            fake_bin,
            windows_home,
            local_msi,
            cmd_log,
            wslpath_log,
            msiexec_log,
        }
    }

    fn command(&self, body: &str) -> Command {
        let helper_path = extracted_helpers(&self.repo);
        let mut command = Command::new("bash");
        command
            .args(["-c", &format!("source \"$1\"\n{body}"), "wsl-test"])
            .arg(helper_path)
            .current_dir(self.repo.path())
            .env("PATH", format!("{}:/usr/bin:/bin", self.fake_bin.display()))
            .env("GIT_AI_LOCAL_MSI", &self.local_msi)
            .env("TEST_WINDOWS_HOME", &self.windows_home)
            .env("TEST_CMD_LOG", &self.cmd_log)
            .env("TEST_WSLPATH_LOG", &self.wslpath_log)
            .env("TEST_MSIEXEC_LOG", &self.msiexec_log);
        command
    }

    fn run(&self, body: &str) -> Output {
        self.command(body).output().unwrap()
    }
}

fn output_text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn log_lines(path: &Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn detects_wsl_without_misclassifying_native_hosts() {
    let fixture = HostFixture::new();
    let output = fixture.run(
        r#"is_wsl_environment linux 5.15.153.1-microsoft-standard-WSL2 ''
is_wsl_environment linux generic-kernel /run/WSL/123_interop
! is_wsl_environment linux generic-kernel ''
! is_wsl_environment macos 5.15.153.1-microsoft-standard-WSL2 /run/WSL/123_interop"#,
    );

    assert!(output.status.success(), "{}", output_text(&output));
}

#[test]
fn selects_existing_msi_assets_and_release_channels() {
    let fixture = HostFixture::new();
    let output = fixture.run(
        r#"test "$(windows_msi_asset x64)" = "git-ai-windows-x64.msi"
test "$(windows_msi_asset arm64)" = "git-ai-windows-arm64.msi"
test "$(windows_msi_download_url git-ai-windows-x64.msi latest)" = "https://github.com/woud420/git-ai/releases/latest/download/git-ai-windows-x64.msi"
test "$(windows_msi_download_url git-ai-windows-arm64.msi v1.2.3)" = "https://github.com/woud420/git-ai/releases/download/v1.2.3/git-ai-windows-arm64.msi""#,
    );

    assert!(output.status.success(), "{}", output_text(&output));
}

#[test]
fn native_installs_do_not_invoke_windows_host_tools() {
    let fixture = HostFixture::new();
    let output = fixture.run(
        r#"if is_wsl_environment linux generic-kernel ''; then
    install_windows_msi_from_wsl x64 local
fi
if is_wsl_environment macos 5.15.153.1-microsoft-standard-WSL2 /run/WSL/123_interop; then
    install_windows_msi_from_wsl x64 local
fi"#,
    );

    assert!(output.status.success(), "{}", output_text(&output));
    assert!(log_lines(&fixture.cmd_log).is_empty());
    assert!(log_lines(&fixture.wslpath_log).is_empty());
    assert!(log_lines(&fixture.msiexec_log).is_empty());
}

#[test]
fn resolves_user_and_translates_the_msi_with_bounded_host_commands() {
    let fixture = HostFixture::new();
    let output = fixture.run("install_windows_msi_from_wsl x64 local");

    assert!(output.status.success(), "{}", output_text(&output));
    assert_eq!(log_lines(&fixture.cmd_log), ["cmd"]);
    let wslpath_calls = log_lines(&fixture.wslpath_log);
    assert_eq!(wslpath_calls.len(), 2);
    assert_eq!(wslpath_calls[0], r"-u C:\Users\Test User");
    assert!(wslpath_calls[1].starts_with("-w "));
    assert!(wslpath_calls[1].contains("/AppData/Local/Temp/git-ai-windows-x64."));

    let msiexec = log_lines(&fixture.msiexec_log);
    assert_eq!(msiexec.iter().filter(|line| *line == "--call--").count(), 1);
    for expected in [
        "/i",
        r"C:\Users\Test User\AppData\Local\Temp\git-ai-windows-x64.msi",
        "/qn",
        "/norestart",
        "API_BASE=https://enterprise.example",
        "API_KEY=test-secret-value",
    ] {
        assert!(
            msiexec.iter().any(|line| line == expected),
            "missing {expected}"
        );
    }
    assert!(!output_text(&output).contains("test-secret-value"));

    let temp_dir = fixture.windows_home.join("AppData/Local/Temp");
    assert!(
        fs::read_dir(temp_dir).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".msi")),
        "the translated MSI staging file must be removed"
    );
}

#[test]
fn reports_missing_windows_interop_without_exposing_environment_values() {
    let fixture = HostFixture::new();
    fs::remove_file(fixture.fake_bin.join("cmd.exe")).unwrap();
    let output = fixture.run("validate_wsl_host_tools");
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("cmd.exe"), "{text}");
    assert!(text.contains("Windows interoperability"), "{text}");
    assert!(!text.contains("test-secret-value"));
}

#[test]
fn rejects_an_unresolved_windows_user_without_echoing_command_output() {
    let fixture = HostFixture::new();
    write_executable(
        &fixture.fake_bin.join("cmd.exe"),
        r#"#!/bin/sh
printf 'unexpected-profile-value\r\n'
"#,
    );
    let output = fixture.run("resolve_windows_user_home");
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("absolute Windows user profile"), "{text}");
    assert!(!text.contains("unexpected-profile-value"));
    assert!(!text.contains("test-secret-value"));
}

#[test]
fn rejects_a_multiline_translated_profile_without_echoing_command_output() {
    let fixture = HostFixture::new();
    write_executable(
        &fixture.fake_bin.join("wslpath"),
        r#"#!/bin/sh
printf '%s\nmalformed-path-value\n' "$TEST_WINDOWS_HOME"
"#,
    );
    let output = fixture.run("install_windows_msi_from_wsl x64 local");
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("invalid Windows user profile path"), "{text}");
    assert!(!text.contains("malformed-path-value"));
    assert!(!text.contains("test-secret-value"));
}

#[test]
fn rejects_a_multiline_translated_msi_path_without_echoing_command_output() {
    let fixture = HostFixture::new();
    write_executable(
        &fixture.fake_bin.join("wslpath"),
        r#"#!/bin/sh
case "$1" in
    -u) printf '%s\n' "$TEST_WINDOWS_HOME" ;;
    -w) printf 'C:\Users\Test User\git-ai.msi\nmalformed-path-value\n' ;;
    *) exit 2 ;;
esac
"#,
    );
    let output = fixture.run("install_windows_msi_from_wsl x64 local");
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("invalid staged MSI path"), "{text}");
    assert!(!text.contains("malformed-path-value"));
    assert!(!text.contains("test-secret-value"));
    assert!(log_lines(&fixture.msiexec_log).is_empty());
}

#[test]
fn suppresses_host_utility_output_when_staging_fails() {
    let fixture = HostFixture::new();
    write_executable(
        &fixture.fake_bin.join("mktemp"),
        r#"#!/bin/sh
printf 'test-secret-value\n' >&2
exit 1
"#,
    );
    let output = fixture.run("install_windows_msi_from_wsl x64 local");
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("could not create a Windows user staging directory"));
    assert!(!text.contains("test-secret-value"));
    assert!(log_lines(&fixture.msiexec_log).is_empty());
}

#[test]
fn installer_failure_reports_only_the_host_exit_code() {
    let fixture = HostFixture::new();
    let mut command = fixture.command("install_windows_msi_from_wsl x64 local");
    command.env("TEST_MSIEXEC_EXIT", "17");
    let output = command.output().unwrap();
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("Windows Installer failed"), "{text}");
    assert!(text.contains("exit code 17"), "{text}");
    assert!(!text.contains("test-secret-value"));
    assert!(!text.contains("enterprise.example"));
}

#[test]
fn repeated_install_skips_an_existing_windows_install() {
    let fixture = HostFixture::new();
    let installed_dir = fixture.windows_home.join(".git-ai/bin");
    let mut first = fixture.command("install_windows_msi_from_wsl x64 local");
    first.env("TEST_INSTALLED_DIR", &installed_dir);
    let first = first.output().unwrap();
    assert!(first.status.success(), "{}", output_text(&first));

    let second = fixture.run("install_windows_msi_from_wsl x64 local");
    assert!(second.status.success(), "{}", output_text(&second));
    assert!(output_text(&second).contains("already installed"));
    assert_eq!(
        log_lines(&fixture.msiexec_log)
            .iter()
            .filter(|line| *line == "--call--")
            .count(),
        1
    );
}

#[test]
fn rejects_an_unsupported_windows_package_architecture() {
    let fixture = HostFixture::new();
    let output = fixture.run("windows_msi_asset riscv64");
    let text = output_text(&output);

    assert!(!output.status.success());
    assert!(text.contains("Unsupported WSL Windows architecture: riscv64"));
}
