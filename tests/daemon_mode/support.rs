use super::repos;

#[path = "completion.rs"]
mod completion;

#[path = "lifecycle.rs"]
mod lifecycle;

#[path = "trace_operations.rs"]
mod trace_operations;

#[path = "reflog_rewrites.rs"]
mod reflog_rewrites;

#[path = "pull_operations.rs"]
mod pull_operations;

#[path = "checkpoint.rs"]
mod checkpoint;

#[path = "trace_listener.rs"]
mod trace_listener;

#[path = "load.rs"]
mod load;

#[path = "memory_watchdog.rs"]
mod memory_watchdog;

#[path = "reingestion.rs"]
mod reingestion;

#[path = "health.rs"]
mod health;

#[path = "http_mock.rs"]
mod http_mock;

#[path = "family_concurrency.rs"]
mod family_concurrency;

#[path = "outbox_replay.rs"]
mod outbox_replay;

#[path = "wltrace.rs"]
mod wltrace;

use git_ai::config::{NotesBackendConfig, NotesBackendKind};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use git_ai::model::checkpoint_delivery::CHECKPOINT_DELIVERY_SCHEMA_VERSION;
#[cfg(unix)]
use git_ai::model::repository::bash_history_db::BashHistoryDatabase;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use git_ai::model::repository::checkpoint_outbox::{
    candidate_roots, decode_delivery, ready_filename,
};
use git_ai::model::working_log::CheckpointKind;
#[cfg(not(windows))]
use git_ai::operations::commands::checkpoint_agent::orchestrator::{
    BaseCommit, CheckpointFile, CheckpointRequest,
};
#[cfg(not(windows))]
use git_ai::operations::daemon::checkpoint::PreparedPathRole;
#[cfg(windows)]
use git_ai::operations::daemon::daemon_log_dir;
use git_ai::operations::daemon::{
    ControlRequest, DaemonConfig, DaemonLock, local_socket_connects_with_timeout,
    open_local_socket_stream_with_timeout, read_daemon_pid, send_control_request,
    send_control_request_with_timeout,
};
use repos::test_file::ExpectedLineExt;
use repos::test_repo::{
    DAEMON_SPAWN_LOADER_RETRY_ATTEMPTS, DAEMON_TEST_PROBE_TIMEOUT,
    DAEMON_TEST_READY_CONTROL_TIMEOUT, DAEMON_TEST_READY_TOTAL_TIMEOUT,
    DaemonTestCompletionLogEntry, DaemonTestScope, RawGitCommand, TestRepo, get_binary_path,
    is_windows_loader_init_failure, real_git_executable,
};
use serde_json::Value;
use serde_json::json;
use serial_test::serial;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ready_checkpoint_outbox_records(repo: &TestRepo) -> Vec<PathBuf> {
    let daemon_config = DaemonConfig::from_home(&repo.daemon_home_path());
    let roots = candidate_roots(
        &daemon_config.internal_dir,
        None,
        &std::env::temp_dir(),
        unsafe { libc::geteuid() },
    )
    .expect("test daemon paths should derive valid checkpoint outbox roots");
    let mut records = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        records.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "ready")
                }),
        );
    }
    records.sort();
    records
}

#[path = "support/startup.rs"]
mod startup;
use startup::*;

#[path = "support/environment.rs"]
mod environment;
use environment::*;

#[path = "support/trace_frames.rs"]
mod trace_frames;
use trace_frames::*;

#[path = "support/api_upload.rs"]
mod api_upload;
use api_upload::*;

#[path = "support/workdir.rs"]
mod workdir;
use workdir::*;
