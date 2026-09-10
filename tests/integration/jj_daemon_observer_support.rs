use super::*;
use crate::repos::test_repo::run_command_output;
use git_ai::model::daemon_control::ControlRequest;
use git_ai::model::repository::sqlite::open_with_flags_and_memory_limits;
use git_ai::operations::daemon::send_control_request_with_timeout;
use git_ai::operations::jj::admission::read_native_admission;
use rusqlite::types::Value as Sql;
use std::path::PathBuf;

pub(super) struct Harness {
    pub(super) fixture: Fixture,
    pub(super) journal: PathBuf,
    pub(super) registration: Json,
}

impl Harness {
    pub(super) fn new(colocated: bool) -> Self {
        Self::from_fixture(Fixture::new(colocated))
    }

    pub(super) fn from_fixture(mut fixture: Fixture) -> Self {
        let root = fixture.root.canonicalize().unwrap();
        fixture.repo.patch_git_ai_config(|patch| {
            patch.allowed_repositories = Some(vec![root.to_str().unwrap().to_owned()]);
        });
        fs::write(fixture.repo.test_home_path().join(".gitconfig"), []).unwrap();
        let journal = fixture.repo.test_home_path().join("registration.sqlite");
        let mut result = Self {
            fixture,
            journal,
            registration: Json::Null,
        };
        let value = result.jj_cli(
            &[
                "initialize".into(),
                "--journal".into(),
                result.journal.to_str().unwrap().into(),
                "--json".into(),
            ],
            true,
        );
        assert_eq!(value["outcome"], "installed");
        result.registration = value["registration"].clone();
        assert_eq!(result.registration["baseline_generation"], 1);
        assert_eq!(result.packet_count(), 0);
        result
    }

    pub(super) fn start(&mut self) {
        assert!(
            std::env::var_os("GIT_AI_TEST_CONFIG_PATCH").is_none(),
            "daemon fixture requires no inherited global config patch"
        );
        self.fixture.repo.start_dedicated_daemon_for_test();
        self.wait(|v| v["runtime"] == "disabled");
        // The readiness probe is traced Git work; drain its side effects before snapshots.
        self.fixture.repo.sync_daemon();
    }

    pub(super) fn target(&self) -> Json {
        let r = &self.registration;
        json!({"source_id":r["source_id"],"initialization_receipt_id":r["initialization_receipt_id"],
            "reader_profile":r["reader_profile"],"baseline_id":r["baseline_id"],"baseline_generation":r["baseline_generation"],
            "workspace_name":r["workspace"]["name"],"attachment_id":r["attachment_id"]})
    }

    pub(super) fn jj_cli(&self, args: &[String], success: bool) -> Json {
        let output = self.output(args);
        assert_eq!(
            output.status.success(),
            success,
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if !success {
            assert_eq!(output.status.code(), Some(1));
        }
        assert!(output.stdout.len() <= 32 * 1024);
        let value: Json = serde_json::from_slice(&output.stdout).unwrap();
        self.metadata(&value);
        value
    }

    fn output(&self, args: &[String]) -> std::process::Output {
        let mut argv = vec!["debug", "jj"];
        argv.extend(args.iter().map(String::as_str));
        let mut command = self
            .fixture
            .repo
            .git_ai_command_without_pre_sync_for_test(&argv, &[]);
        let cwd = if args.first().is_some_and(|arg| arg == "observer")
            && args.get(1).is_some_and(|arg| arg != "enable")
        {
            self.fixture.repo.test_home_path()
        } else {
            &self.fixture.root
        };
        command
            .current_dir(cwd)
            .env("GIT_AI_DEBUG", "0")
            .env("GIT_CONFIG_COUNT", "0")
            .env_remove("GIT_CONFIG_PARAMETERS")
            .env_remove("GIT_CONFIG")
            .env_remove("GIT_DIR")
            .env_remove("GIT_COMMON_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_TRACE2_EVENT")
            .env_remove("GIT_AI");
        run_command_output(&mut command, "jj observer fixture CLI").unwrap()
    }

    pub(super) fn metadata(&self, value: &Json) {
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["backend"], "jj");
        assert_eq!(value["attribution_enabled"], false);
        let text = value.to_string();
        for path in [self.fixture.repo.path(), self.fixture.repo.test_home_path()] {
            assert!(
                !text.contains(path.to_str().unwrap()),
                "metadata exposed a locator"
            );
        }
        for key in [
            "operation_bytes",
            "view_bytes",
            "journal_locator",
            "workspace_locator",
        ] {
            assert!(
                !text.contains(&format!("\"{key}\"")),
                "metadata exposed {key}"
            );
        }
    }

    pub(super) fn control(&self, action: &str, success: bool) -> Json {
        let mut args = vec!["observer".into(), action.into(), "--json".into()];
        if action == "enable" {
            args.extend(["--journal".into(), self.journal.to_str().unwrap().into()]);
        }
        self.jj_cli(&args, success)
    }

    pub(super) fn ready_control(&self, action: &str) -> Json {
        let until = Instant::now() + Duration::from_secs(30);
        for _ in 0..60 {
            let mut args = vec!["observer".into(), action.into(), "--json".into()];
            if action == "enable" {
                args.extend(["--journal".into(), self.journal.to_str().unwrap().into()]);
            }
            let output = self.output(&args);
            assert!(output.stdout.len() <= 32 * 1024);
            let value: Json = serde_json::from_slice(&output.stdout).unwrap();
            self.metadata(&value);
            if output.status.success() {
                assert_eq!(value["action"], format!("observer_{action}"));
                return value;
            }
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(value["error"]["code"], "observer_busy", "{value}");
            assert!(Instant::now() < until, "observer stayed busy: {value}");
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("bounded observer control retry count exhausted")
    }

    pub(super) fn status(&self) -> Json {
        let request: ControlRequest =
            serde_json::from_value(json!({"method":"jj.observer.status.v1"})).unwrap();
        let reply = send_control_request_with_timeout(
            &self.fixture.repo.daemon_control_socket_path(),
            &request,
            Duration::from_millis(500),
        )
        .unwrap();
        assert!(reply.ok, "{reply:?}");
        let value = reply.data.unwrap();
        self.metadata(&value);
        assert_eq!(value["action"], "observer_status");
        value
    }

    pub(super) fn wait(&self, predicate: impl Fn(&Json) -> bool) -> Json {
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let value = self.status();
            if predicate(&value) {
                return value;
            }
            assert!(
                Instant::now() < until,
                "observer condition timed out: {value}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub(super) fn active(&self, generation: u64, heads: &[String]) -> Json {
        let value = self.wait(|v| {
            v["runtime"] == "active"
                && v["session_cursor"]["generation"] == generation
                && v["in_flight"] == false
        });
        let mut heads = heads.to_vec();
        heads.sort();
        assert_eq!(
            value["session_cursor"],
            json!({"generation":generation,"admitted_head_ids":heads})
        );
        assert_eq!(value["target"], self.target());
        assert_eq!(value["desired_intent"], "enabled");
        assert!(value["last_error"].is_null());
        value
    }

    pub(super) fn disabled(&self) -> Json {
        let request = self.control("disable", true);
        assert_eq!(request["desired_intent"], "disabled");
        let value = self.wait(|v| v["runtime"] == "disabled" && v["in_flight"] == false);
        assert!(value["session_cursor"].is_null());
        value
    }

    pub(super) fn blocked(&self, code: &str) -> Json {
        let value = self.wait(|v| {
            v["runtime"] == "blocked"
                && v["in_flight"] == false
                && v["last_error"]["persisted"] == true
        });
        assert_eq!(value["last_error"]["code"], code, "{value}");
        let message = value["last_error"]["message"].as_str().unwrap();
        assert!(!message.is_empty() && message.len() <= 4096);
        assert_eq!(value["target"], self.target());
        value
    }

    pub(super) fn sql(&self) -> rusqlite::Connection {
        open_with_flags_and_memory_limits(&self.journal, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap()
    }

    pub(super) fn rows(&self, tables: &[&str]) -> Vec<Vec<Vec<Sql>>> {
        let conn = self.sql();
        tables
            .iter()
            .map(|table| {
                let mut query = conn
                    .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2 LIMIT 17"))
                    .unwrap();
                let width = query.column_count();
                let rows = query
                    .query_map([], |row| {
                        (0..width)
                            .map(|i| row.get(i))
                            .collect::<rusqlite::Result<Vec<Sql>>>()
                    })
                    .unwrap()
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .unwrap();
                assert!(rows.len() < 17);
                rows
            })
            .collect()
    }

    pub(super) fn saved_rows(&self) -> Vec<Vec<Vec<Sql>>> {
        self.rows(&[
            "jj_native_baselines",
            "jj_native_sources",
            "jj_native_registrations",
            "jj_native_workspaces",
        ])
    }

    pub(super) fn admission_rows(&self) -> Vec<Vec<Vec<Sql>>> {
        self.rows(&["jj_native_admissions", "jj_native_admission_states"])
    }

    pub(super) fn packet_count(&self) -> u64 {
        self.sql()
            .query_row("SELECT COUNT(*) FROM jj_native_admissions", [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    pub(super) fn packet(&self, generation: u64) -> DurableNativeAdmission {
        let id: String = self.sql().query_row("SELECT admission_id FROM jj_native_admissions WHERE source_id=?1 AND generation=?2", rusqlite::params![self.registration["source_id"].as_str().unwrap(),generation], |row| row.get(0)).unwrap();
        let journal = JjObservationJournal::open_read_only_at_path(&self.journal).unwrap();
        let packet = read_native_admission(
            &journal,
            self.registration["source_id"].as_str().unwrap(),
            &id,
            Instant::now() + Duration::from_secs(10),
            &mut ReadBudget::new(48 * 1024 * 1024),
        )
        .unwrap()
        .unwrap();
        let receipt = packet.receipt();
        assert_eq!(receipt.generation(), generation);
        assert_eq!(
            receipt.source_id(),
            self.registration["source_id"].as_str().unwrap()
        );
        assert_eq!(
            receipt.initialization_receipt_id(),
            self.registration["initialization_receipt_id"]
                .as_str()
                .unwrap()
        );
        assert_eq!(
            receipt.baseline_id(),
            self.registration["baseline_id"].as_str().unwrap()
        );
        assert_eq!(receipt.baseline_generation(), 1);
        packet
    }

    pub(super) fn manual_capture(&self, generation: u64, heads: &[String]) -> Json {
        let mut args = vec![
            "capture".into(),
            "--json".into(),
            "--journal".into(),
            self.journal.to_str().unwrap().into(),
            "--expect-generation".into(),
            generation.to_string(),
        ];
        for (flag, key) in [
            ("--expect-source", "source_id"),
            (
                "--expect-initialization-receipt",
                "initialization_receipt_id",
            ),
            ("--expect-baseline", "baseline_id"),
        ] {
            args.extend([flag.into(), self.registration[key].as_str().unwrap().into()]);
        }
        for head in heads {
            args.extend(["--expect-head".into(), head.clone()]);
        }
        let result = self.jj_cli(&args, true);
        assert_eq!(result["outcome"], "admitted");
        result
    }

    pub(super) fn baseline_heads(&self) -> Vec<String> {
        serde_json::from_value(self.registration["captured_head_ids"].clone()).unwrap()
    }

    pub(super) fn select(&self, record: &JjOperationEvidence) {
        verify_evidence(JJ_OBSERVATION_READER_PROFILE, record).unwrap();
        self.fixture.write_evidence(record);
        self.fixture.set_heads(&[&record.operation_id]);
        self.fixture.write_checkout(&record.operation_id, "default");
        capture_current_state(&self.fixture.context(), deadline()).unwrap();
    }
}
