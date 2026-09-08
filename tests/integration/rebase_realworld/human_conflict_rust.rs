use super::{
    ExpectedLineExt, TestRepo, assert_note_base_commit_matches, assert_note_files_exact, fs,
    get_commit_chain,
};

/// Test 2: Rust lib.rs — feature adds AI parser functions, main edits the same
/// mod declaration at the top → conflict on C2 (middle of chain).
/// C1' is attributed normally; C2' loses lib.rs; C3'–C5' accumulate helpers.rs,
/// types.rs, error.rs as expected.
#[test]
fn test_human_conflict_rust_lib_c2_conflicts_surroundings_ok() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("src/lib.rs", "pub mod parser;\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: changes the mod declaration → conflicts with feature C2's edit
    repo.commit_untracked_file(
        "src/lib.rs",
        "pub mod parser;\npub mod types;\n",
        "main: add types mod",
    );
    repo.commit_untracked_file("src/main.rs", "fn main() {}\n", "main: add main.rs");
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "README.md",
        "# mylib\nA Rust library.\n",
        "main: add README",
    );
    repo.commit_untracked_file(".github/workflows/ci.yml",
        "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps: [{uses: actions/checkout@v3}]\n",
        "main: add CI workflow",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI adds parser/tokenize to a separate file
    let mut tokenizer = repo.filename("src/tokenizer.rs");
    tokenizer.set_contents(crate::lines![
        "pub enum Token { Ident(String), Number(i64), Eof }".ai(),
        "".ai(),
        "pub fn tokenize(input: &str) -> Vec<Token> {".ai(),
        "    input.split_whitespace()".ai(),
        "        .map(|w| Token::Ident(w.to_string()))".ai(),
        "        .collect()".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add tokenizer").unwrap();

    // C2: AI edits lib.rs to export tokenizer — WILL CONFLICT with main's mod change
    let mut lib = repo.filename("src/lib.rs");
    lib.replace_at(0, "pub mod tokenizer;".ai());
    repo.stage_all_and_commit("feat: C2 export tokenizer in lib.rs")
        .unwrap();

    // C3: AI adds helpers.rs
    let mut helpers = repo.filename("src/helpers.rs");
    helpers.set_contents(crate::lines![
        "pub fn is_digit(c: char) -> bool { c.is_ascii_digit() }".ai(),
        "pub fn is_alpha(c: char) -> bool { c.is_alphabetic() }".ai(),
        "pub fn is_whitespace(c: char) -> bool { c.is_whitespace() }".ai(),
        "pub fn to_lowercase(s: &str) -> String { s.to_lowercase() }".ai(),
        "pub fn trim_quotes(s: &str) -> &str { s.trim_matches('\"') }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add helpers").unwrap();

    // C4: AI adds types.rs
    let mut types = repo.filename("src/types.rs");
    types.set_contents(crate::lines![
        "#[derive(Debug, Clone, PartialEq)]".ai(),
        "pub struct Span { pub start: usize, pub end: usize }".ai(),
        "".ai(),
        "#[derive(Debug)]".ai(),
        "pub enum ParseError {".ai(),
        "    UnexpectedToken(String),".ai(),
        "    UnexpectedEof,".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add types").unwrap();

    // C5: AI adds error.rs
    let mut error = repo.filename("src/error.rs");
    error.set_contents(crate::lines![
        "use std::fmt;".ai(),
        "use crate::types::ParseError;".ai(),
        "".ai(),
        "impl fmt::Display for ParseError {".ai(),
        "    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {".ai(),
        "        match self {".ai(),
        "            ParseError::UnexpectedToken(t) => write!(f, \"unexpected: {}\", t),".ai(),
        "            ParseError::UnexpectedEof => write!(f, \"unexpected EOF\"),".ai(),
        "        }".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add error Display impl")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/lib.rs at C2"
    );

    // Human resolves by keeping both mods
    fs::write(
        repo.path().join("src/lib.rs"),
        "pub mod parser;\npub mod types;\npub mod tokenizer;\n",
    )
    .unwrap();
    repo.git(&["add", "src/lib.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': tokenizer.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/tokenizer.rs"]);

    // C2': lib.rs human-resolved conflict — all AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &[]);

    // C3': helpers.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/helpers.rs"]);

    // C4': types.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/types.rs"]);

    // C5': error.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/error.rs"]);
}

/// Test 5: Rust src/config.rs — main and feature both extend a constants block,
/// triggering a conflict on C2.  C1' has config.rs attributed; C2' loses it
/// due to human resolution; C3'–C5' accumulate cache.rs, retry.rs, timeout.rs.
#[test]
fn test_human_conflict_rust_config_c2_loses_attribution_rest_accumulate() {
    let repo = TestRepo::new();

    repo.commit_untracked_file(
        "src/config.rs",
        "pub const MAX_CONN: u32 = 10;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds another constant → conflicts with feature's C2 edit
    repo.commit_untracked_file(
        "src/config.rs",
        "pub const MAX_CONN: u32 = 10;\npub const TIMEOUT_MS: u64 = 5000;\n",
        "main: add TIMEOUT_MS constant",
    );
    repo.commit_untracked_file("src/pool.rs",
        "pub struct Pool { size: u32 }\nimpl Pool { pub fn new(size: u32) -> Self { Pool { size } } }\n",
        "main: add connection pool",
    );
    repo.commit_untracked_file(
        "src/metrics.rs",
        "pub fn record_latency(ms: u64) { eprintln!(\"latency: {}ms\", ms); }\n",
        "main: add metrics",
    );
    repo.commit_untracked_file(
        "src/health.rs",
        "pub fn is_healthy() -> bool { true }\n",
        "main: add health check",
    );
    repo.commit_untracked_file(
        "src/shutdown.rs",
        "pub fn graceful_shutdown() { eprintln!(\"shutting down\"); }\n",
        "main: add shutdown handler",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates a separate file (no conflict — config.rs not touched)
    let mut defaults = repo.filename("src/defaults.rs");
    defaults.set_contents(crate::lines!["pub const DEFAULT_POOL_SIZE: u32 = 5;".ai(),]);
    repo.stage_all_and_commit("feat: C1 add defaults.rs")
        .unwrap();

    // C2: AI edits config.rs to add IDLE_TIMEOUT — WILL CONFLICT with main's TIMEOUT_MS
    let mut config = repo.filename("src/config.rs");
    config.set_contents(crate::lines![
        "pub const MAX_CONN: u32 = 10;",
        "pub const IDLE_TIMEOUT_MS: u64 = 30_000;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add IDLE_TIMEOUT_MS to config")
        .unwrap();

    // C3: AI creates cache.rs
    let mut cache = repo.filename("src/cache.rs");
    cache.set_contents(crate::lines![
        "use std::collections::HashMap;".ai(),
        "pub struct Cache<K, V>(HashMap<K, V>);".ai(),
        "impl<K: Eq + std::hash::Hash, V> Cache<K, V> {".ai(),
        "    pub fn new() -> Self { Cache(HashMap::new()) }".ai(),
        "    pub fn get(&self, k: &K) -> Option<&V> { self.0.get(k) }".ai(),
        "    pub fn set(&mut self, k: K, v: V) { self.0.insert(k, v); }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Cache struct")
        .unwrap();

    // C4: AI creates retry.rs
    let mut retry = repo.filename("src/retry.rs");
    retry.set_contents(crate::lines![
        "use crate::config::MAX_RETRIES;".ai(),
        "pub fn with_retry<T, E>(mut f: impl FnMut() -> Result<T, E>) -> Result<T, E> {".ai(),
        "    let mut last = f();".ai(),
        "    for _ in 1..MAX_RETRIES {".ai(),
        "        if last.is_ok() { return last; }".ai(),
        "        last = f();".ai(),
        "    }".ai(),
        "    last".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add retry helper")
        .unwrap();

    // C5: AI creates timeout.rs
    let mut timeout_file = repo.filename("src/timeout.rs");
    timeout_file.set_contents(crate::lines![
        "use std::time::{Duration, Instant};".ai(),
        "".ai(),
        "pub fn run_with_timeout<T>(duration: Duration, f: impl FnOnce() -> T) -> Option<T> {".ai(),
        "    let start = Instant::now();".ai(),
        "    let result = f();".ai(),
        "    if start.elapsed() <= duration { Some(result) } else { None }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add timeout runner")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/config.rs at C2"
    );

    // Human resolves: keep all constants
    fs::write(
        repo.path().join("src/config.rs"),
        "pub const MAX_CONN: u32 = 10;\npub const TIMEOUT_MS: u64 = 5000;\npub const IDLE_TIMEOUT_MS: u64 = 30_000;\n",
    ).unwrap();
    repo.git(&["add", "src/config.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': defaults.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/defaults.rs"]);

    // C2': config.rs human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &[]);

    // C3': cache.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/cache.rs"]);

    // C4': retry.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/retry.rs"]);

    // C5': timeout.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/timeout.rs"]);
}

/// Test 7: Rust src/server.rs — feature adds AI HTTP handler functions; main
/// adds a conflicting use declaration in C4.  C1'–C3' and C5' keep their AI
/// attribution; C4' (server.rs) is dropped due to human resolution.
#[test]
fn test_human_conflict_rust_server_c4_human_resolved_c5_accumulates() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("src/server.rs", "pub fn start() {}\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: adds a use statement that conflicts with feature C4's edit
    repo.commit_untracked_file(
        "src/server.rs",
        "use std::net::TcpListener;\npub fn start() {}\n",
        "main: add TcpListener import",
    );
    repo.commit_untracked_file(
        "src/router.rs",
        "pub struct Router;\nimpl Router { pub fn new() -> Self { Router } }\n",
        "main: add router",
    );
    repo.commit_untracked_file(
        "src/response.rs",
        "pub struct Response { pub status: u16, pub body: String }\n",
        "main: add Response type",
    );
    repo.commit_untracked_file(
        "src/request.rs",
        "pub struct Request { pub path: String, pub method: String }\n",
        "main: add Request type",
    );
    repo.commit_untracked_file(
        "src/middleware.rs",
        "pub trait Middleware { fn handle(&self, req: &str) -> String; }\n",
        "main: add Middleware trait",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates src/handler.rs
    let mut handler = repo.filename("src/handler.rs");
    handler.set_contents(crate::lines![
        "pub fn handle_get(path: &str) -> String {".ai(),
        "    format!(\"GET {} OK\", path)".ai(),
        "}".ai(),
        "".ai(),
        "pub fn handle_post(path: &str, body: &str) -> String {".ai(),
        "    format!(\"POST {} body={}\", path, body)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add GET/POST handlers")
        .unwrap();

    // C2: AI creates src/router_ext.rs
    let mut router_ext = repo.filename("src/router_ext.rs");
    router_ext.set_contents(crate::lines![
        "use std::collections::HashMap;".ai(),
        "pub type HandlerFn = fn(&str) -> String;".ai(),
        "pub struct RouteMap(HashMap<String, HandlerFn>);".ai(),
        "impl RouteMap {".ai(),
        "    pub fn new() -> Self { RouteMap(HashMap::new()) }".ai(),
        "    pub fn register(&mut self, path: &str, h: HandlerFn) { self.0.insert(path.to_string(), h); }".ai(),
        "    pub fn dispatch(&self, path: &str) -> Option<String> { self.0.get(path).map(|h| h(path)) }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add RouteMap").unwrap();

    // C3: AI creates src/static_files.rs
    let mut statics = repo.filename("src/static_files.rs");
    statics.set_contents(crate::lines![
        "use std::path::Path;".ai(),
        "pub fn serve_static(path: &str) -> Option<Vec<u8>> {".ai(),
        "    let p = Path::new(path);".ai(),
        "    if p.exists() { std::fs::read(p).ok() } else { None }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add static file server")
        .unwrap();

    // C4: AI edits server.rs to add bind — WILL CONFLICT with main's use std::net::TcpListener
    let mut server = repo.filename("src/server.rs");
    server.replace_at(
        0,
        "pub fn start() { let _l = std::net::TcpListener::bind(\"0.0.0.0:8080\"); }".ai(),
    );
    repo.stage_all_and_commit("feat: C4 add bind in server start")
        .unwrap();

    // C5: AI creates src/tls.rs
    let mut tls = repo.filename("src/tls.rs");
    tls.set_contents(crate::lines![
        "pub struct TlsConfig { pub cert_path: String, pub key_path: String }".ai(),
        "impl TlsConfig {".ai(),
        "    pub fn new(cert: &str, key: &str) -> Self {".ai(),
        "        TlsConfig { cert_path: cert.into(), key_path: key.into() }".ai(),
        "    }".ai(),
        "    pub fn is_valid(&self) -> bool {".ai(),
        "        std::path::Path::new(&self.cert_path).exists()".ai(),
        "            && std::path::Path::new(&self.key_path).exists()".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add TLS config struct")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/server.rs at C4"
    );

    // Human resolves by combining import and function body
    fs::write(
        repo.path().join("src/server.rs"),
        "use std::net::TcpListener;\npub fn start() { let _l = TcpListener::bind(\"0.0.0.0:8080\"); }\n",
    ).unwrap();
    repo.git(&["add", "src/server.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': handler.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/handler.rs"]);

    // C2': router_ext.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/router_ext.rs"]);

    // C3': static_files.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/static_files.rs"]);

    // C4': human-resolved conflict on server.rs.  The human changed `std::net::TcpListener`
    // to `TcpListener` in the resolution — the line content differs from the original AI
    // line so content-based mapping finds no match. Note metadata is preserved but no
    // file attestations remain.
    let c4_note = repo.read_authorship_note(&chain[3]);
    assert!(
        c4_note.is_some(),
        "c4: note metadata should survive conflict rebase"
    );
    assert_note_files_exact(&repo, &chain[3], "c4_files", &[]);

    // C5': tls.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/tls.rs"]);
}

crate::reuse_tests_in_worktree!(
    test_human_conflict_rust_lib_c2_conflicts_surroundings_ok,
    test_human_conflict_rust_config_c2_loses_attribution_rest_accumulate,
    test_human_conflict_rust_server_c4_human_resolved_c5_accumulates,
);
