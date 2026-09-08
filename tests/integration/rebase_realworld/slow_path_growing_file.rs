use super::{
    ExpectedLineExt, TestRepo, assert_blame_sample_at_commit, assert_note_base_commit_matches,
    assert_note_files_exact, get_commit_chain,
};

/// Test 5: 10-commit feature branch, all appending to src/engine.rs.
/// Upstream prepends a 3-line license header. Verifies ALL 10 SHAs.
/// Critical: sha0 must NOT have sha9's accepted_lines.
#[test]
fn test_slow_path_growing_shared_file_10_commits() {
    let repo = TestRepo::new();

    // Initial: src/engine.rs with trailing newline
    repo.commit_untracked_file("src/engine.rs", "// Engine core\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: prepend 3-line license header (forces slow path)
    repo.commit_untracked_file("src/engine.rs",
        "// Copyright 2024 MyOrg\n// Licensed under MIT License\n// See LICENSE file for details\n\n// Engine core\n",
        "main: prepend license header to engine.rs",
    );
    repo.commit_untracked_file(
        "src/error.rs",
        "#[derive(Debug)]\npub enum EngineError { NotFound, InvalidInput, Timeout }\n",
        "main: add engine errors",
    );
    repo.commit_untracked_file("src/config.rs",
        "pub struct EngineConfig { pub workers: usize, pub stack_size: usize }\nimpl Default for EngineConfig { fn default() -> Self { Self { workers: 4, stack_size: 2 * 1024 * 1024 } } }\n",
        "main: add engine config",
    );
    repo.commit_untracked_file(
        "benches/engine_bench.rs",
        "fn main() { /* bench placeholder */ }\n",
        "main: add bench placeholder",
    );
    repo.commit_untracked_file(
        "tests/engine_test.rs",
        "#[test]\nfn smoke_test() { assert!(true); }\n",
        "main: add smoke test",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 8 AI lines to engine.rs
    let mut eng = repo.filename("src/engine.rs");
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine {".ai(),
        "    running: bool,".ai(),
        "    workers: usize,".ai(),
        "}".ai(),
        "impl Engine {".ai(),
        "    pub fn new(workers: usize) -> Self { Self { running: false, workers } }".ai(),
        "    pub fn start(&mut self) { self.running = true; }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add Engine struct")
        .unwrap();

    // C2: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "impl Engine {".ai(),
        "    pub fn new(workers: usize) -> Self { Self { running: false, workers } }".ai(),
        "    pub fn start(&mut self) { self.running = true; }".ai(),
        "    pub fn stop(&mut self) { self.running = false; }".ai(),
        "    pub fn is_running(&self) -> bool { self.running }".ai(),
        "}".ai(),
        "".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "impl Task {".ai(),
        "    pub fn new(id: u64, payload: Vec<u8>) -> Self { Self { id, payload } }".ai(),
        "    pub fn size(&self) -> usize { self.payload.len() }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Task struct")
        .unwrap();

    // C3: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "impl Engine { pub fn new(workers: usize) -> Self { Self { running: false, workers } } pub fn start(&mut self) { self.running = true; } }".ai(),
        "".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "impl Task { pub fn new(id: u64, payload: Vec<u8>) -> Self { Self { id, payload } } }".ai(),
        "".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "impl Queue {".ai(),
        "    pub fn new() -> Self { Self { tasks: Default::default() } }".ai(),
        "    pub fn push(&mut self, t: Task) { self.tasks.push_back(t); }".ai(),
        "    pub fn pop(&mut self) -> Option<Task> { self.tasks.pop_front() }".ai(),
        "    pub fn len(&self) -> usize { self.tasks.len() }".ai(),
        "    pub fn is_empty(&self) -> bool { self.tasks.is_empty() }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Queue struct")
        .unwrap();

    // C4: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "impl Engine { pub fn new(w: usize) -> Self { Self { running: false, workers: w } } pub fn start(&mut self) { self.running = true; } }".ai(),
        "".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "impl Task { pub fn new(id: u64, payload: Vec<u8>) -> Self { Self { id, payload } } }".ai(),
        "".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "impl Queue { pub fn new() -> Self { Self { tasks: Default::default() } } pub fn push(&mut self, t: Task) { self.tasks.push_back(t); } pub fn pop(&mut self) -> Option<Task> { self.tasks.pop_front() } }".ai(),
        "".ai(),
        "pub struct Worker { pub id: usize }".ai(),
        "impl Worker {".ai(),
        "    pub fn new(id: usize) -> Self { Self { id } }".ai(),
        "    pub fn execute(&self, task: &Task) -> Result<(), String> {".ai(),
        "        if task.payload.is_empty() { return Err(\"empty payload\".into()); }".ai(),
        "        Ok(())".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Worker struct")
        .unwrap();

    // C5: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "impl Engine { pub fn new(w: usize) -> Self { Self { running: false, workers: w } } pub fn start(&mut self) { self.running = true; } }".ai(),
        "".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "impl Task { pub fn new(id: u64, payload: Vec<u8>) -> Self { Self { id, payload } } }".ai(),
        "".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "impl Queue { pub fn push(&mut self, t: Task) { self.tasks.push_back(t); } }".ai(),
        "".ai(),
        "pub struct Worker { pub id: usize }".ai(),
        "impl Worker { pub fn execute(&self, task: &Task) -> Result<(), String> { Ok(()) } }".ai(),
        "".ai(),
        "pub struct Scheduler { queue: Queue, workers: Vec<Worker> }".ai(),
        "impl Scheduler {".ai(),
        "    pub fn new(n: usize) -> Self { Self { queue: Queue { tasks: Default::default() }, workers: (0..n).map(Worker::new).collect() } }".ai(),
        "    pub fn submit(&mut self, task: Task) { self.queue.push(task); }".ai(),
        "    pub fn worker_count(&self) -> usize { self.workers.len() }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add Scheduler struct")
        .unwrap();

    // C6: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "impl Engine { pub fn new(w: usize) -> Self { Self { running: false, workers: w } } pub fn start(&mut self) { self.running = true; } }".ai(),
        "".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "pub struct Worker { pub id: usize }".ai(),
        "pub struct Scheduler { queue: Queue, workers: Vec<Worker> }".ai(),
        "".ai(),
        "pub struct Metrics {".ai(),
        "    tasks_submitted: u64,".ai(),
        "    tasks_completed: u64,".ai(),
        "    tasks_failed: u64,".ai(),
        "}".ai(),
        "impl Metrics {".ai(),
        "    pub fn new() -> Self { Self { tasks_submitted: 0, tasks_completed: 0, tasks_failed: 0 } }".ai(),
        "    pub fn record_submit(&mut self) { self.tasks_submitted += 1; }".ai(),
        "    pub fn record_complete(&mut self) { self.tasks_completed += 1; }".ai(),
        "    pub fn record_fail(&mut self) { self.tasks_failed += 1; }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C6 add Metrics struct")
        .unwrap();

    // C7: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "pub struct Worker { pub id: usize }".ai(),
        "pub struct Scheduler { queue: Queue, workers: Vec<Worker> }".ai(),
        "pub struct Metrics { tasks_submitted: u64, tasks_completed: u64, tasks_failed: u64 }".ai(),
        "".ai(),
        "pub struct RateLimit {".ai(),
        "    capacity: u64,".ai(),
        "    tokens: u64,".ai(),
        "    refill_rate: u64,".ai(),
        "}".ai(),
        "impl RateLimit {".ai(),
        "    pub fn new(capacity: u64, refill_rate: u64) -> Self { Self { capacity, tokens: capacity, refill_rate } }".ai(),
        "    pub fn try_consume(&mut self, n: u64) -> bool { if self.tokens >= n { self.tokens -= n; true } else { false } }".ai(),
        "    pub fn refill(&mut self) { self.tokens = (self.tokens + self.refill_rate).min(self.capacity); }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C7 add RateLimit struct")
        .unwrap();

    // C8: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "pub struct Scheduler { queue: Queue, workers: Vec<Worker> }".ai(),
        "pub struct Metrics { tasks_submitted: u64, tasks_completed: u64 }".ai(),
        "pub struct RateLimit { capacity: u64, tokens: u64, refill_rate: u64 }".ai(),
        "".ai(),
        "pub struct CircuitBreaker {".ai(),
        "    state: BreakState,".ai(),
        "    failures: u32,".ai(),
        "    threshold: u32,".ai(),
        "}".ai(),
        "pub enum BreakState { Closed, Open, HalfOpen }".ai(),
        "impl CircuitBreaker {".ai(),
        "    pub fn new(threshold: u32) -> Self { Self { state: BreakState::Closed, failures: 0, threshold } }".ai(),
        "    pub fn is_open(&self) -> bool { matches!(self.state, BreakState::Open) }".ai(),
        "    pub fn record_failure(&mut self) { self.failures += 1; if self.failures >= self.threshold { self.state = BreakState::Open; } }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C8 add CircuitBreaker")
        .unwrap();

    // C9: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "pub struct Scheduler { queue: Queue, workers: Vec<Worker> }".ai(),
        "pub struct Metrics { tasks_submitted: u64, tasks_completed: u64 }".ai(),
        "pub struct RateLimit { capacity: u64, tokens: u64, refill_rate: u64 }".ai(),
        "pub struct CircuitBreaker { state: BreakState, failures: u32, threshold: u32 }".ai(),
        "pub enum BreakState { Closed, Open, HalfOpen }".ai(),
        "".ai(),
        "pub struct HealthCheck {".ai(),
        "    checks: Vec<Box<dyn Fn() -> bool + Send + Sync>>,".ai(),
        "}".ai(),
        "impl HealthCheck {".ai(),
        "    pub fn new() -> Self { Self { checks: Vec::new() } }".ai(),
        "    pub fn add<F: Fn() -> bool + Send + Sync + 'static>(&mut self, f: F) { self.checks.push(Box::new(f)); }".ai(),
        "    pub fn all_healthy(&self) -> bool { self.checks.iter().all(|f| f()) }".ai(),
        "    pub fn healthy_count(&self) -> usize { self.checks.iter().filter(|f| f()).count() }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C9 add HealthCheck struct")
        .unwrap();

    // C10: append 8 more AI lines
    eng.set_contents(crate::lines![
        "// Engine core",
        "".ai(),
        "pub struct Engine { running: bool, workers: usize }".ai(),
        "pub struct Task { pub id: u64, pub payload: Vec<u8> }".ai(),
        "pub struct Queue { tasks: std::collections::VecDeque<Task> }".ai(),
        "pub struct Scheduler { queue: Queue, workers: Vec<Worker> }".ai(),
        "pub struct Metrics { tasks_submitted: u64, tasks_completed: u64 }".ai(),
        "pub struct RateLimit { capacity: u64, tokens: u64, refill_rate: u64 }".ai(),
        "pub struct CircuitBreaker { state: BreakState, failures: u32, threshold: u32 }".ai(),
        "pub enum BreakState { Closed, Open, HalfOpen }".ai(),
        "pub struct HealthCheck { checks: Vec<Box<dyn Fn() -> bool + Send + Sync>> }".ai(),
        "".ai(),
        "pub struct Tracer {".ai(),
        "    spans: Vec<(String, std::time::Duration)>,".ai(),
        "}".ai(),
        "impl Tracer {".ai(),
        "    pub fn new() -> Self { Self { spans: Vec::new() } }".ai(),
        "    pub fn record(&mut self, name: impl Into<String>, duration: std::time::Duration) {"
            .ai(),
        "        self.spans.push((name.into(), duration));".ai(),
        "    }".ai(),
        "    pub fn spans(&self) -> &[(String, std::time::Duration)] { &self.spans }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C10 add Tracer struct")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 10);

    // Verify ALL 10 SHAs
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["src/engine.rs"]);

    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/engine.rs",
        "sha1_blame_new",
        &[("pub struct Task {", true), ("impl Task {", true)],
    );

    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/engine.rs",
        "sha2_blame_new",
        &[("pub struct Queue {", true), ("impl Queue {", true)],
    );

    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/engine.rs",
        "sha3_blame_new",
        &[("pub struct Worker {", true), ("impl Worker {", true)],
    );

    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/engine.rs",
        "sha4_blame_new",
        &[("pub struct Scheduler {", true), ("impl Scheduler {", true)],
    );

    assert_note_base_commit_matches(&repo, &chain[5], "sha5");
    assert_note_files_exact(&repo, &chain[5], "sha5_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[5],
        "src/engine.rs",
        "sha5_blame_new",
        &[("pub struct Metrics {", true), ("impl Metrics {", true)],
    );

    assert_note_base_commit_matches(&repo, &chain[6], "sha6");
    assert_note_files_exact(&repo, &chain[6], "sha6_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[6],
        "src/engine.rs",
        "sha6_blame_new",
        &[("pub struct RateLimit {", true), ("impl RateLimit {", true)],
    );

    assert_note_base_commit_matches(&repo, &chain[7], "sha7");
    assert_note_files_exact(&repo, &chain[7], "sha7_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[7],
        "src/engine.rs",
        "sha7_blame_new",
        &[
            ("pub struct CircuitBreaker {", true),
            ("pub enum BreakState {", true),
        ],
    );

    assert_note_base_commit_matches(&repo, &chain[8], "sha8");
    assert_note_files_exact(&repo, &chain[8], "sha8_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[8],
        "src/engine.rs",
        "sha8_blame_new",
        &[
            ("pub struct HealthCheck {", true),
            ("impl HealthCheck {", true),
        ],
    );

    assert_note_base_commit_matches(&repo, &chain[9], "sha9");
    assert_note_files_exact(&repo, &chain[9], "sha9_files", &["src/engine.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[9],
        "src/engine.rs",
        "sha9_blame_new",
        &[("pub struct Tracer {", true), ("impl Tracer {", true)],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_growing_shared_file_10_commits,);
