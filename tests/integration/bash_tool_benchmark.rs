//! Benchmarks for the bash tool stat-snapshot and diff system.
//!
//! Measures end-to-end `handle_bash_tool` latency (PreToolUse + PostToolUse)
//! across synthetic repos of varying sizes.  Each test spins up a dedicated
//! isolated daemon instance so watermarks are clean and the system-wide daemon
//! is never touched.
//!
//! | Repo Size | Files   | Target Pre-hook P95 | Target Post-hook P95 |
//! |-----------|---------|---------------------|----------------------|
//! | Small     | 1,000   | < 15ms              | < 15ms               |
//! | Medium    | 10,000  | < 75ms              | < 75ms               |
//! | Large     | 100,000 | < 750ms             | < 750ms              |
//! | XLarge    | 500,000 | < 7.5s              | < 7.5s               |
//!
//! Run with: cargo test bash_tool_benchmark --release -- --nocapture --ignored

use crate::test_utils::DurationStatistics;
use git_ai::model::working_log::AgentId;
use git_ai::operations::commands::checkpoint_agent::bash_tool;
use git_ai::operations::daemon::control_api::ControlRequest;
use git_ai::operations::daemon::send_control_request_with_timeout;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Per-test isolated daemon
// ---------------------------------------------------------------------------

/// Spawns an isolated `git-ai bg run` daemon for benchmarking and kills it on
/// drop.  Sets `GIT_AI_DAEMON_CONTROL_SOCKET` in the current process so that
/// `query_daemon_watermarks` inside `handle_bash_tool` connects to this daemon
/// instead of the system-wide one.
struct BenchDaemon {
    child: Child,
    control_socket: PathBuf,
    /// Saved value of GIT_AI_DAEMON_CONTROL_SOCKET before we overwrote it.
    prev_socket_env: Option<String>,
}

impl BenchDaemon {
    fn start(repo_root: &Path, daemon_home: &Path) -> Self {
        let control_socket = daemon_home.join("control.sock");
        let trace_socket = daemon_home.join("trace.sock");
        let test_db = daemon_home.join("test.db");

        fs::create_dir_all(daemon_home).expect("failed to create daemon_home");

        // Resolve the binary: prefer the release build used by the benchmark
        // runner, fall back to debug.
        let binary = {
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let rel = manifest.join("target/release/git-ai");
            let dbg = manifest.join("target/debug/git-ai");
            if rel.exists() { rel } else { dbg }
        };

        let child = Command::new(&binary)
            .args(["bg", "run"])
            .current_dir(repo_root)
            .env("GIT_AI_DAEMON_HOME", daemon_home)
            .env("GIT_AI_DAEMON_CONTROL_SOCKET", &control_socket)
            .env("GIT_AI_DAEMON_TRACE_SOCKET", &trace_socket)
            .env("GIT_AI_TEST_DB_PATH", &test_db)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn bench daemon");

        // Wait up to 5 s for the socket to become reachable.
        let probe = ControlRequest::StatusFamily {
            repo_working_dir: repo_root.to_string_lossy().into_owned(),
        };
        let mut ready = false;
        for _ in 0..200 {
            if send_control_request_with_timeout(&control_socket, &probe, Duration::from_millis(25))
                .is_ok()
            {
                ready = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(ready, "bench daemon did not become ready within 5s");

        // Point the in-process query at this daemon's socket.
        let prev_socket_env = std::env::var("GIT_AI_DAEMON_CONTROL_SOCKET").ok();
        // SAFETY: benchmark tests run single-threaded with #[ignore]; no other
        // threads read this env var concurrently during the test.
        unsafe { std::env::set_var("GIT_AI_DAEMON_CONTROL_SOCKET", &control_socket) };

        BenchDaemon {
            child,
            control_socket,
            prev_socket_env,
        }
    }
}

impl Drop for BenchDaemon {
    fn drop(&mut self) {
        // Restore env var.
        // SAFETY: same single-threaded guarantee as in start().
        unsafe {
            match &self.prev_socket_env {
                Some(v) => std::env::set_var("GIT_AI_DAEMON_CONTROL_SOCKET", v),
                None => std::env::remove_var("GIT_AI_DAEMON_CONTROL_SOCKET"),
            }
        }
        // Graceful shutdown, then hard kill.
        let _ = send_control_request_with_timeout(
            &self.control_socket,
            &ControlRequest::Shutdown,
            Duration::from_millis(500),
        );
        std::thread::sleep(Duration::from_millis(200));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Statistics helpers
// ---------------------------------------------------------------------------

/// Timing data for one iteration of a full pre-hook + post-hook round trip.
#[derive(Debug, Clone)]
struct IterationTiming {
    /// Time for handle_bash_tool(PreToolUse): cleanup + snapshot walk + JSON write.
    pre_hook_duration: Duration,
    /// Time for handle_bash_tool(PostToolUse): JSON read + snapshot walk + diff.
    post_hook_duration: Duration,
}

/// Descriptive statistics for a set of duration measurements.
#[derive(Debug)]
struct DurationStats {
    count: usize,
    min: Duration,
    max: Duration,
    average: Duration,
    p95: Duration,
    std_dev_ms: f64,
}

impl DurationStats {
    fn from_durations(durations: &[Duration]) -> Self {
        let stats = DurationStatistics::from_durations(durations);
        assert!(stats.count() > 0, "cannot compute stats from empty slice");

        Self {
            count: stats.count(),
            min: stats.min().unwrap(),
            max: stats.max().unwrap(),
            average: stats.average().unwrap(),
            p95: stats.percentile_nearest_rank(0.95).unwrap(),
            std_dev_ms: stats.std_dev_ms().unwrap(),
        }
    }

    fn print(&self, label: &str) {
        println!("\n=== {} ({} runs) ===", label, self.count);
        println!("  Min:      {:.2}ms", self.min.as_secs_f64() * 1000.0);
        println!("  Average:  {:.2}ms", self.average.as_secs_f64() * 1000.0);
        println!("  Max:      {:.2}ms", self.max.as_secs_f64() * 1000.0);
        println!("  P95:      {:.2}ms", self.p95.as_secs_f64() * 1000.0);
        println!("  Std Dev:  {:.2}ms", self.std_dev_ms);
    }
}

// ---------------------------------------------------------------------------
// Synthetic repo construction
// ---------------------------------------------------------------------------

/// Create a temporary git repo at `root` containing `file_count` files spread
/// across a nested directory tree.  Files are grouped into directories of at
/// most ~100 files each, with up to 3 levels of nesting for realism.
fn create_synthetic_repo(root: &Path, file_count: usize) {
    fs::create_dir_all(root).expect("failed to create repo root");

    // git init
    let output = Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .expect("git init failed");
    assert!(output.status.success(), "git init failed");

    // Configure user for commits
    for (key, val) in [
        ("user.name", "Bench User"),
        ("user.email", "bench@test.com"),
    ] {
        let output = Command::new("git")
            .args(["config", key, val])
            .current_dir(root)
            .output()
            .expect("git config failed");
        assert!(output.status.success(), "git config {} failed", key);
    }

    // Create a .gitignore to mimic real repos (ignore build artifacts, etc.)
    fs::write(root.join(".gitignore"), "target/\nnode_modules/\n*.o\n")
        .expect("failed to write .gitignore");

    // Build a nested directory tree.
    // Strategy: files_per_dir ~= 100, dirs are nested up to 3 levels.
    let files_per_dir: usize = 100;
    let total_dirs = file_count.div_ceil(files_per_dir);

    let mut files_created: usize = 0;
    for dir_index in 0..total_dirs {
        // Compute a nested path: level0/level1/level2
        let l0 = dir_index % 50;
        let l1 = (dir_index / 50) % 50;
        let l2 = dir_index / 2500;
        let dir_path = root
            .join(format!("src_{}", l2))
            .join(format!("mod_{}", l1))
            .join(format!("pkg_{}", l0));
        fs::create_dir_all(&dir_path).expect("failed to create nested dir");

        let remaining = file_count - files_created;
        let batch = remaining.min(files_per_dir);
        for file_index in 0..batch {
            let filename = format!("file_{}.rs", file_index);
            let content = format!(
                "// auto-generated benchmark file {}/{}\nfn f{}() {{}}\n",
                dir_index,
                file_index,
                files_created + file_index
            );
            fs::write(dir_path.join(&filename), content).expect("failed to write file");
        }
        files_created += batch;
    }

    assert_eq!(
        files_created, file_count,
        "expected to create {} files, created {}",
        file_count, files_created
    );

    // Stage and commit everything.  For large repos, `git add -A` followed by
    // a single commit is the fastest approach.
    let add_output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .output()
        .expect("git add failed");
    assert!(add_output.status.success(), "git add -A failed");

    let commit_output = Command::new("git")
        .args(["commit", "-m", "initial synthetic commit"])
        .current_dir(root)
        .output()
        .expect("git commit failed");
    assert!(
        commit_output.status.success(),
        "git commit failed: {}",
        String::from_utf8_lossy(&commit_output.stderr)
    );

    // Backdate .git/index by 30s so all setup files are covered by the
    // git-index-mtime watermark proxy.  Files written after this call have
    // mtimes ~30s newer — well outside the 2s MTIME_GRACE_WINDOW — so they
    // appear in snapshots without needing any sleep.
    let git_index = root.join(".git").join("index");
    filetime::set_file_mtime(
        &git_index,
        filetime::FileTime::from_unix_time(filetime::FileTime::now().unix_seconds() - 30, 0),
    )
    .expect("failed to backdate .git/index");
}

// ---------------------------------------------------------------------------
// Benchmark harness
// ---------------------------------------------------------------------------

const NUM_ITERATIONS: usize = 5;

/// Run `NUM_ITERATIONS` of a full pre-hook + post-hook round trip on the given
/// repo root.  Each iteration calls `handle_bash_tool` for both events, which
/// exercises the complete user-visible latency path:
///   PreToolUse:  stale-snapshot cleanup + snapshot walk + JSON write to disk
///   PostToolUse: JSON read from disk + snapshot walk + in-memory diff
///
/// The daemon watermark query fails fast (no daemon in tests), so the snapshot
/// always performs a full walk — the cold/no-daemon worst case.
///
/// Returns (pre_hook_stats, post_hook_stats).
fn run_benchmark(repo_root: &Path, label: &str) -> (DurationStats, DurationStats) {
    println!(
        "\n--- {} benchmark ({} iterations) ---",
        label, NUM_ITERATIONS
    );

    let mut timings: Vec<IterationTiming> = Vec::with_capacity(NUM_ITERATIONS);
    let session_id = "bench-session";

    for i in 1..=NUM_ITERATIONS {
        let tool_use_id = format!("bench-call-{}", i);

        let agent_id = AgentId {
            tool: "bench".to_string(),
            id: "bench".to_string(),
            model: String::new(),
        };

        // Pre-hook: snapshot walk + daemon send
        let pre_start = Instant::now();
        bash_tool::handle_bash_pre_tool_use_with_context(
            repo_root,
            session_id,
            &tool_use_id,
            &agent_id,
            None,
            "t_test123456789a",
            None,
        )
        .expect("pre-hook should succeed");
        let pre_hook_duration = pre_start.elapsed();

        // Modify a single file between hooks to make the diff non-trivial
        let marker_path = repo_root.join("bench_marker.txt");
        fs::write(&marker_path, format!("iteration {}", i)).expect("failed to write marker");

        // Post-hook: daemon query + snapshot walk + in-memory diff
        let post_start = Instant::now();
        let result = bash_tool::handle_bash_post_tool_use(
            repo_root,
            session_id,
            &tool_use_id,
            &agent_id,
            None,
            "t_test123456789a",
            None,
        )
        .expect("post-hook should succeed");
        let post_hook_duration = post_start.elapsed();

        // Sanity: the marker file must appear as a change
        assert!(
            !matches!(result.action, bash_tool::BashCheckpointAction::NoChanges),
            "post-hook should detect marker file change"
        );

        println!(
            "  Iteration {}: pre={:.2}ms, post={:.2}ms",
            i,
            pre_hook_duration.as_secs_f64() * 1000.0,
            post_hook_duration.as_secs_f64() * 1000.0,
        );

        timings.push(IterationTiming {
            pre_hook_duration,
            post_hook_duration,
        });

        // Clean up marker for next iteration
        let _ = fs::remove_file(&marker_path);
    }

    let pre_durations: Vec<Duration> = timings.iter().map(|t| t.pre_hook_duration).collect();
    let post_durations: Vec<Duration> = timings.iter().map(|t| t.post_hook_duration).collect();

    let pre_stats = DurationStats::from_durations(&pre_durations);
    let post_stats = DurationStats::from_durations(&post_durations);

    pre_stats.print(&format!("{} Pre-hook", label));
    post_stats.print(&format!("{} Post-hook", label));

    (pre_stats, post_stats)
}

mod diff_and_fallback;
mod snapshot_scale;
