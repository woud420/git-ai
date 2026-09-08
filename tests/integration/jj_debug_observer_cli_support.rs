use crate::debug_context::snapshot;
use crate::jj_debug_cli::error;
use crate::repos::test_repo::{DaemonTestScope, TestRepo, run_command_output};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Output};

pub(super) const ACTIONS: [&str; 4] = ["enable", "status", "disable", "resume"];

pub(super) struct Fixture {
    repo: TestRepo,
    untouched_home: PathBuf,
    missing_journal: String,
    repo_before: BTreeMap<PathBuf, Vec<u8>>,
    home_before: BTreeMap<PathBuf, Vec<u8>>,
}

impl Fixture {
    pub fn new() -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let untouched_home = repo.test_home_path().join("never-opened-observer-home");
        let missing_journal = untouched_home
            .join("journal.sqlite")
            .to_str()
            .unwrap()
            .to_owned();
        let repo_before = snapshot(repo.path());
        let home_before = snapshot(repo.test_home_path());
        Self {
            repo,
            untouched_home,
            missing_journal,
            repo_before,
            home_before,
        }
    }

    pub fn journal(&self) -> String {
        self.missing_journal.clone()
    }

    pub fn args<'a>(&'a self, action: &'a str) -> Vec<&'a str> {
        let mut args = vec!["observer", action];
        if action == "enable" {
            args.extend(["--journal", &self.missing_journal]);
        }
        args.push("--json");
        args
    }

    pub fn command(&self, args: &[&str]) -> Command {
        let mut all = vec!["debug", "jj"];
        all.extend_from_slice(args);
        let mut command = self
            .repo
            .git_ai_command_without_pre_sync_for_test(&all, &[]);
        command
            .env("GIT_AI_DEBUG", "0")
            .env_remove("GIT_AI_TEST_ALLOW_DAEMON_AUTOSPAWN")
            .env("_GITAI_INTERNAL_DISABLE_WRAPPER_DAEMON_AUTOSPAWN", "1");
        command
    }

    pub fn poisoned(&self, args: &[&str]) -> Command {
        let mut command = self.command(args);
        command
            .env("HOME", &self.untouched_home)
            .env("USERPROFILE", &self.untouched_home)
            .env("GIT_AI_DAEMON_HOME", &self.untouched_home)
            .env("GIT_AI_TEST_CONFIG_PATCH", "not-json")
            .env("GIT_CONFIG_GLOBAL", self.untouched_home.join("gitconfig"))
            .env(
                "GIT_AI_DAEMON_CONTROL_SOCKET",
                self.untouched_home.join("control.sock"),
            )
            .env(
                "GIT_AI_DAEMON_TRACE_SOCKET",
                self.untouched_home.join("trace.sock"),
            );
        command
    }

    pub fn check(&self, output: Output, code: &str) {
        error(output, code);
        self.preserved();
    }

    pub fn rejected(&self, args: &[&str]) {
        self.check(
            run_command_output(&mut self.poisoned(args), "observer rejected arguments").unwrap(),
            "usage",
        );
    }

    pub fn accepted(&self, args: &[&str]) {
        let code = if cfg!(any(target_os = "linux", target_os = "macos")) {
            "daemon_unavailable"
        } else {
            "unsupported_platform"
        };
        self.check(
            run_command_output(&mut self.command(args), "observer accepted arguments").unwrap(),
            code,
        );
    }

    pub fn preserved(&self) {
        assert!(!self.untouched_home.exists());
        assert_eq!(snapshot(self.repo.path()), self.repo_before);
        assert_eq!(snapshot(self.repo.test_home_path()), self.home_before);
    }
}
