use super::*;
use ciborium::Value;
use rusqlite::types::Value as SqlValue;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CASE_ENV: &str = "GIT_AI_JJ_REGISTRATION_CASE";
const CHILD: &str = "jj_capture::registration::registration_case_child";
pub const REMOTE: &str = "https://github.com/jj-registration-fixture/excluded.git";
pub const TABLES: [&str; 4] = [
    "jj_native_baselines",
    "jj_native_sources",
    "jj_native_registrations",
    "jj_native_workspaces",
];

#[derive(Clone, Copy)]
pub enum Policy {
    Root,
    Empty,
    OtherRoot,
    RootWithRemoteExclusion,
    Remote,
    EscapedRoot,
}

#[derive(Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub root: PathBuf,
    pub repository: PathBuf,
    pub repo_dir: PathBuf,
    pub test_home: PathBuf,
    pub journal_path: PathBuf,
    patch: String,
    excluded: Vec<String>,
}

pub fn run_case(name: &str, colocated: bool, policy: Policy) {
    let mut fixture = Fixture::new(colocated);
    run_fixture_case(name, &mut fixture, policy, CHILD);
}

pub fn run_fixture_case(name: &str, fixture: &mut Fixture, policy: Policy, child_filter: &str) {
    let root = fixture.root.canonicalize().unwrap();
    let allowed = match policy {
        Policy::Root | Policy::RootWithRemoteExclusion => vec![root.clone()],
        Policy::Empty => vec![],
        Policy::OtherRoot => vec![root.join("unrelated-policy-root")],
        Policy::EscapedRoot => vec![root.join("allowed")],
        Policy::Remote => vec![],
    };
    fixture.repo.patch_git_ai_config(|patch| {
        patch.allowed_repositories = Some(if matches!(policy, Policy::Remote) {
            vec![REMOTE.to_owned()]
        } else {
            allowed
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        });
    });
    let excluded = if matches!(policy, Policy::RootWithRemoteExclusion) {
        vec![REMOTE.to_owned()]
    } else {
        vec![]
    };
    let config_path = fixture.repo.test_home_path().join(".git-ai/config.json");
    let mut file_config: git_ai::config::FileConfig =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    file_config.exclude_repositories = Some(excluded.clone());
    fs::write(&config_path, serde_json::to_vec(&file_config).unwrap()).unwrap();
    let test_home = fixture.repo.test_home_path().canonicalize().unwrap();
    // TestRepo redirects this optional Git config path but does not create it.
    fs::write(test_home.join(".gitconfig"), []).unwrap();
    let case = Case {
        name: name.to_owned(),
        root,
        repository: fixture.repo.path().canonicalize().unwrap(),
        repo_dir: fixture.repo_dir.canonicalize().unwrap(),
        journal_path: test_home.join("registration.sqlite"),
        test_home,
        patch: fixture.repo.config_patch_json().unwrap(),
        excluded,
    };
    let template = fixture
        .repo
        .git_ai_command_without_pre_sync_for_test(&[], &[]);
    let mut command = Command::new(std::env::current_exe().unwrap());
    for (key, value) in template.get_envs() {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    command
        .args([
            "--ignored",
            "--exact",
            child_filter,
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CASE_ENV, serde_json::to_string(&case).unwrap())
        .env("GIT_CONFIG_COUNT", "0")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env_remove("GIT_CONFIG")
        .env_remove("GIT_DIR")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_TRACE2_EVENT")
        .env_remove("GIT_AI")
        .current_dir(&case.repository)
        .stdin(Stdio::null());
    let stdout = case.test_home.join("registration-child.stdout");
    let stderr = case.test_home.join("registration-child.stderr");
    command.stdout(fs::File::create(&stdout).unwrap());
    command.stderr(fs::File::create(&stderr).unwrap());
    let mut child = command.spawn().unwrap();
    let until = Instant::now() + Duration::from_secs(60);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("failed to poll registration child: {error}");
            }
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "registration child timed out: {}{}",
                bounded_text(&stdout),
                bounded_text(&stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = format!("{}{}", bounded_text(&stdout), bounded_text(&stderr));
    assert!(status.success(), "{name}: {output}");
    assert!(
        output.contains(&format!("REGISTRATION_CASE_COMPLETED:{name}")),
        "child filter did not execute: {output}"
    );
}

fn bounded_text(path: &Path) -> String {
    let file = fs::File::open(path).unwrap();
    let mut bytes = Vec::new();
    file.take(1024 * 1024).read_to_end(&mut bytes).unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn child_case() -> Option<Case> {
    let value = std::env::var(CASE_ENV).ok()?;
    assert!(value.len() <= 128 * 1024);
    Some(serde_json::from_str(&value).unwrap())
}

impl Case {
    pub fn config(&self) -> Config {
        assert_eq!(
            PathBuf::from(std::env::var_os("HOME").unwrap())
                .canonicalize()
                .unwrap(),
            self.test_home
        );
        assert_eq!(
            std::env::var("GIT_AI_TEST_CONFIG_PATCH").unwrap(),
            self.patch
        );
        assert!(self.journal_path.starts_with(&self.test_home));
        assert!(self.root.starts_with(&self.repository));
        assert!(self.repo_dir.starts_with(&self.root));
        assert_eq!(
            PathBuf::from(std::env::var_os("GIT_CONFIG_GLOBAL").unwrap())
                .canonicalize()
                .unwrap(),
            self.test_home.join(".gitconfig").canonicalize().unwrap()
        );
        assert_eq!(std::env::var("GIT_CONFIG_NOSYSTEM").unwrap(), "1");
        let config = Config::fresh();
        assert_eq!(
            serde_json::to_value(&config).unwrap()["exclude_repositories"],
            serde_json::json!(self.excluded)
        );
        config
    }

    pub fn context(&self) -> WorkspaceContext {
        discover(&self.root).unwrap()
    }
    pub fn open(&self) -> JjObservationJournal {
        JjObservationJournal::open_at_path(&self.journal_path).unwrap()
    }
    pub fn namespace(&self) -> PathBuf {
        self.repo_dir.join("git-ai")
    }
    pub fn seal(&self) -> PathBuf {
        self.namespace().join("registration")
    }
    pub fn write_evidence(&self, evidence: &JjOperationEvidence) {
        write_evidence_to(&self.repo_dir, evidence);
    }
    pub fn write_checkout(&self, operation: &str) {
        fs::write(
            self.root.join(".jj/working_copy/checkout"),
            checkout_bytes(operation, "default"),
        )
        .unwrap();
    }
    pub fn heads(&self, ids: &[&str]) {
        let directory = self.repo_dir.join("op_heads/heads");
        let entries: Vec<_> = fs::read_dir(&directory).unwrap().take(36).collect();
        assert!(entries.len() < 36);
        for entry in entries {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        for id in ids {
            fs::write(directory.join(id), []).unwrap();
        }
    }
    pub fn sql(&self) -> rusqlite::Connection {
        open_with_memory_limits(&self.journal_path).unwrap()
    }
    pub fn register(
        &self,
        journal: &mut JjObservationJournal,
        config: &Config,
    ) -> Result<JjRegistrationOutcome, JjRegistrationError> {
        register_current_state(journal, &self.context(), config, deadline(), &mut budget())
    }
    pub fn reopen(
        &self,
        journal: &JjObservationJournal,
        config: &Config,
    ) -> Result<Option<RegisteredJjCurrentState>, JjRegistrationError> {
        reopen_registered_current_state(journal, &self.context(), config, deadline(), &mut budget())
    }
}

pub fn budget() -> ReadBudget {
    ReadBudget::new(17 * 1024 * 1024)
}
pub fn installed(value: JjRegistrationOutcome) -> RegisteredJjCurrentState {
    match value {
        JjRegistrationOutcome::Installed(value) => value,
        JjRegistrationOutcome::AlreadyRegistered(_) => panic!("expected first installation"),
    }
}
pub fn already(value: JjRegistrationOutcome) -> RegisteredJjCurrentState {
    match value {
        JjRegistrationOutcome::AlreadyRegistered(value) => value,
        JjRegistrationOutcome::Installed(_) => panic!("retry installed a new registration"),
    }
}
pub fn error<T>(value: Result<T, JjRegistrationError>) -> JjRegistrationError {
    match value {
        Ok(_) => panic!("expected registration to be unavailable"),
        Err(error) => {
            assert!(!error.to_string().is_empty());
            error
        }
    }
}
pub fn seal_bytes(source: &str) -> Vec<u8> {
    format!("git-ai/jj/source-seal/v1\nsource_id={source}\nreader_profile={JJ_OBSERVATION_READER_PROFILE}\n").into_bytes()
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn hex_id(id: &str) {
    assert_eq!(id.len(), 64);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
}
pub fn text_field<'a>(value: &'a Value, name: &str) -> &'a str {
    let Value::Map(fields) = value else {
        panic!("expected record map")
    };
    fields
        .iter()
        .find(|(key, _)| key.as_text() == Some(name))
        .unwrap()
        .1
        .as_text()
        .unwrap()
}

#[derive(Debug, PartialEq)]
pub struct State {
    files: BTreeMap<PathBuf, Entry>,
    config_files: [Option<Vec<u8>>; 2],
    rows: Vec<(String, Vec<Vec<SqlValue>>)>,
}
pub fn state(case: &Case) -> State {
    let conn = case.sql();
    let mut rows = Vec::new();
    for table in TABLES.into_iter().chain([
        "jj_sources",
        "jj_operations",
        "jj_views",
        "jj_batches",
        "schema_metadata",
    ]) {
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY 1, 2 LIMIT 17"))
            .unwrap();
        let columns = statement.column_count();
        let values = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|i| row.get::<_, SqlValue>(i))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(values.len() < 17);
        rows.push((table.to_owned(), values));
    }
    State {
        files: manifest(&case.repository),
        config_files: [
            fs::read(case.test_home.join(".gitconfig")).ok(),
            fs::read(case.test_home.join(".git-ai/config.json")).ok(),
        ],
        rows,
    }
}
pub fn counts(case: &Case) -> [usize; 4] {
    let conn = case.sql();
    TABLES.map(|table| {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    })
}
pub fn reject_unchanged(case: &Case, journal: &mut JjObservationJournal, config: &Config) {
    let before = state(case);
    error(case.reopen(journal, config));
    assert_eq!(state(case), before);
    error(case.register(journal, config));
    assert_eq!(state(case), before);
}
