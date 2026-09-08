use crate::debug_context::snapshot;
use crate::jj_debug_cli::error;
use crate::repos::test_repo::{DaemonTestScope, TestRepo};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Output};

pub(super) struct Fixture {
    pub repo: TestRepo,
    parent: PathBuf,
    before: BTreeMap<PathBuf, Vec<u8>>,
}

impl Fixture {
    pub fn new() -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let parent = repo.test_home_path().join("never-created-observe-journal");
        let before = snapshot(repo.path());
        Self {
            repo,
            parent,
            before,
        }
    }

    pub fn args(&self) -> Vec<String> {
        vec![
            "observe".into(),
            "--journal".into(),
            self.parent.join("journal.sqlite").to_str().unwrap().into(),
            "--json".into(),
            "--expect-source".into(),
            "11".repeat(32),
            "--expect-initialization-receipt".into(),
            "22".repeat(32),
            "--expect-baseline".into(),
            "33".repeat(32),
            "--expect-generation".into(),
            "0".into(),
            "--expect-head".into(),
            "44".repeat(64),
            "--expect-workspace".into(),
            "default".into(),
            "--expect-attachment".into(),
            "55".repeat(32),
        ]
    }

    pub fn command(&self, args: &[String]) -> Command {
        let mut all = vec!["debug", "jj"];
        all.extend(args.iter().map(String::as_str));
        self.repo
            .git_ai_command_without_pre_sync_for_test(&all, &[])
    }

    pub fn check(&self, output: Output, code: &str) {
        error(output, code);
        self.preserved();
    }

    pub fn rejected(&self, args: &[String]) {
        self.check(self.command(args).output().unwrap(), "usage");
    }

    pub fn syntax_accepted(&self, args: &[String]) {
        let code = if cfg!(any(target_os = "linux", target_os = "macos")) {
            "not_jj_workspace"
        } else {
            "unsupported_platform"
        };
        self.check(self.command(args).output().unwrap(), code);
    }

    pub fn preserved(&self) {
        assert!(!self.parent.exists());
        assert_eq!(snapshot(self.repo.path()), self.before);
    }
}

pub(super) fn replace(args: &mut [String], flag: &str, value: impl Into<String>) {
    let index = args.iter().position(|arg| arg == flag).unwrap();
    args[index + 1] = value.into();
}

pub(super) fn remove(args: &mut Vec<String>, flag: &str) {
    let index = args.iter().position(|arg| arg == flag).unwrap();
    drop(args.drain(index..index + 2));
}

pub(super) fn option(args: &mut Vec<String>, flag: &str, value: &str) {
    args.extend([flag.to_owned(), value.to_owned()]);
}
