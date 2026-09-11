use super::environment::{configure_test_daemon_env, configure_test_home_env};
use super::{Command, DaemonConfig, Path, PathBuf, RawGitCommand, TestRepo, fs, get_binary_path};

#[derive(Clone)]
pub(super) struct WorkdirRaceHarness {
    pub(super) test_home: PathBuf,
    pub(super) test_db_path: PathBuf,
    pub(super) daemon_home: PathBuf,
    pub(super) control_socket_path: PathBuf,
    pub(super) trace_socket_path: PathBuf,
}

impl WorkdirRaceHarness {
    pub(super) fn new(repo: &TestRepo, trace_socket_path: PathBuf) -> Self {
        Self {
            test_home: repo.test_home_path().to_path_buf(),
            test_db_path: repo.test_db_path().to_path_buf(),
            daemon_home: repo.daemon_home_path(),
            control_socket_path: repo.daemon_control_socket_path(),
            trace_socket_path,
        }
    }

    pub(super) fn run_traced_git(&self, workdir: &Path, args: &[&str]) {
        let output = RawGitCommand::in_working_dir(workdir, args)
            .configure(|command| configure_test_home_env(command, &self.test_home))
            .env("GIT_AI_TEST_DB_PATH", &self.test_db_path)
            .env("GITAI_TEST_DB_PATH", &self.test_db_path)
            .env(
                "GIT_TRACE2_EVENT",
                DaemonConfig::trace2_event_target_for_path(&self.trace_socket_path),
            )
            .env("GIT_TRACE2_EVENT_NESTING", "0")
            .output()
            .expect("failed to execute traced git command");
        assert!(
            output.status.success(),
            "traced git command failed in {}: git {} \nstdout:{}\nstderr:{}",
            workdir.display(),
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(super) fn run_delegated_checkpoint(&self, workdir: &Path, file_rel: &str) {
        let mut command = Command::new(get_binary_path());
        command
            .args(["checkpoint", "mock_ai", file_rel])
            .current_dir(workdir);
        configure_test_home_env(&mut command, &self.test_home);
        configure_test_daemon_env(
            &mut command,
            &self.daemon_home,
            &self.control_socket_path,
            &self.trace_socket_path,
        );
        let output = command
            .env("GIT_AI_TEST_DB_PATH", &self.test_db_path)
            .env("GITAI_TEST_DB_PATH", &self.test_db_path)
            .env("GIT_AI_DAEMON_CHECKPOINT_DELEGATE", "true")
            .output()
            .expect("failed to execute delegated checkpoint");
        assert!(
            output.status.success(),
            "delegated checkpoint failed in {} for {} \nstdout:{}\nstderr:{}",
            workdir.display(),
            file_rel,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(super) fn write_ai_line_checkpoint_and_add(
        &self,
        workdir: &Path,
        file_rel: &str,
        line: &str,
    ) {
        fs::write(workdir.join(file_rel), format!("{line}\n"))
            .expect("failed writing ai line test file");
        self.run_delegated_checkpoint(workdir, file_rel);
        self.run_traced_git(workdir, &["add", file_rel]);
    }
}

pub(super) fn parse_blame_line(line: &str) -> (String, String) {
    if let Some(start_paren) = line.find('(')
        && let Some(end_paren) = line.find(')')
    {
        let author_section = &line[start_paren + 1..end_paren];
        let content = line[end_paren + 1..].trim().to_string();

        let parts: Vec<&str> = author_section.split_whitespace().collect();
        let mut author_parts = Vec::new();
        for part in parts {
            if part.chars().next().unwrap_or('a').is_ascii_digit() {
                break;
            }
            author_parts.push(part);
        }
        return (author_parts.join(" "), content);
    }
    ("unknown".to_string(), line.trim().to_string())
}

pub(super) fn is_ai_author(author: &str) -> bool {
    let author_lower = author.to_lowercase();
    author_lower.contains("mock_ai")
        || author_lower.contains("claude")
        || author_lower.contains("cursor")
        || author_lower.contains("codex")
}

pub(super) fn assert_blame_lines_for_workdir(
    repo: &TestRepo,
    workdir: &Path,
    file_rel: &str,
    expected: &[(String, bool)],
) {
    let blame_output = repo
        .git_ai_from_working_dir(workdir, &["blame", file_rel])
        .unwrap_or_else(|e| {
            panic!(
                "git-ai blame failed in {} for {}: {}",
                workdir.display(),
                file_rel,
                e
            )
        });
    let actual: Vec<(String, String)> = blame_output
        .lines()
        .filter(|line: &&str| !line.trim().is_empty())
        .map(parse_blame_line)
        .collect();
    assert_eq!(
        actual.len(),
        expected.len(),
        "line count mismatch for {} in {}\nblame:\n{}",
        file_rel,
        workdir.display(),
        blame_output
    );

    for (idx, ((author, content), (expected_content, expected_ai))) in
        actual.iter().zip(expected.iter()).enumerate()
    {
        assert_eq!(
            content,
            expected_content,
            "line {} content mismatch for {} in {}",
            idx + 1,
            file_rel,
            workdir.display()
        );
        let actual_ai = is_ai_author(author);
        assert_eq!(
            actual_ai,
            *expected_ai,
            "line {} attribution mismatch for {} in {} (author='{}', line='{}')",
            idx + 1,
            file_rel,
            workdir.display(),
            author,
            content
        );
    }
}

pub(super) fn assert_single_ai_line_for_workdir(
    repo: &TestRepo,
    workdir: &Path,
    file_rel: &str,
    line: &str,
) {
    assert_blame_lines_for_workdir(repo, workdir, file_rel, &[(line.to_string(), true)]);
}
