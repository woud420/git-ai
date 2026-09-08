use super::{
    ExpectedLineExt, TestRepo, assert_blame_sample_at_commit, assert_note_base_commit_matches,
    assert_note_files_exact, assert_note_no_forbidden_files, get_commit_chain,
};

/// Test 2: Rust lib.rs — upstream prepends crate-level doc and deny(warnings),
/// feature appends impl blocks per commit AND adds a unique module file.
/// Checks cumulative file sets and that future module files don't leak.
#[test]
fn test_slow_path_rust_lib_rs_main_prepends_feature_adds_impls() {
    let repo = TestRepo::new();

    // Initial: src/lib.rs with trailing newline
    repo.commit_untracked_file("src/lib.rs", "pub mod types;\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: prepend crate-level docs + deny(warnings)
    repo.commit_untracked_file(
        "src/lib.rs",
        "//! Library crate\n#![deny(warnings)]\n\npub mod types;\n",
        "main: prepend crate docs and deny(warnings)",
    );
    repo.commit_untracked_file(
        "src/error.rs",
        "pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;\n",
        "main: add error types",
    );
    repo.commit_untracked_file(
        "build.rs",
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
        "main: add build script",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "README.md",
        "# mylib\n\nA Rust library.\n",
        "main: add README",
    );

    // Feature branch from initial commit (before main's prepend)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append impl Block to lib.rs + create mod_a.rs
    let mut lib = repo.filename("src/lib.rs");
    lib.set_contents(crate::lines![
        "pub mod types;",
        "".ai(),
        "pub struct Cache {".ai(),
        "    inner: std::collections::HashMap<String, Vec<u8>>,".ai(),
        "}".ai(),
        "impl Cache {".ai(),
        "    pub fn new() -> Self { Self { inner: Default::default() } }".ai(),
        "    pub fn get(&self, key: &str) -> Option<&Vec<u8>> { self.inner.get(key) }".ai(),
        "    pub fn set(&mut self, key: impl Into<String>, val: Vec<u8>) { self.inner.insert(key.into(), val); }".ai(),
        "}".ai(),
    ]);
    let mut mod_a = repo.filename("src/mod_a.rs");
    mod_a.set_contents(crate::lines![
        "pub fn encode_base64(input: &[u8]) -> String {".ai(),
        "    use std::fmt::Write;".ai(),
        "    let alphabet = b\"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/\";"
            .ai(),
        "    let mut out = String::new();".ai(),
        "    for chunk in input.chunks(3) {".ai(),
        "        let _ = write!(out, \"{}\", alphabet[(chunk[0] >> 2) as usize] as char);".ai(),
        "    }".ai(),
        "    out".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add Cache impl + mod_a")
        .unwrap();

    // C2: append Config impl to lib.rs + create mod_b.rs
    lib.set_contents(crate::lines![
        "pub mod types;",
        "".ai(),
        "pub struct Cache {".ai(),
        "    inner: std::collections::HashMap<String, Vec<u8>>,".ai(),
        "}".ai(),
        "impl Cache {".ai(),
        "    pub fn new() -> Self { Self { inner: Default::default() } }".ai(),
        "    pub fn get(&self, key: &str) -> Option<&Vec<u8>> { self.inner.get(key) }".ai(),
        "    pub fn set(&mut self, key: impl Into<String>, val: Vec<u8>) { self.inner.insert(key.into(), val); }".ai(),
        "}".ai(),
        "".ai(),
        "pub struct Config {".ai(),
        "    pub max_connections: usize,".ai(),
        "    pub timeout_ms: u64,".ai(),
        "}".ai(),
        "impl Default for Config {".ai(),
        "    fn default() -> Self { Self { max_connections: 10, timeout_ms: 5000 } }".ai(),
        "}".ai(),
        "impl Config {".ai(),
        "    pub fn with_timeout(mut self, ms: u64) -> Self { self.timeout_ms = ms; self }".ai(),
        "}".ai(),
    ]);
    let mut mod_b = repo.filename("src/mod_b.rs");
    mod_b.set_contents(crate::lines![
        "use std::time::{Duration, Instant};".ai(),
        "pub struct Timer { start: Instant }".ai(),
        "impl Timer {".ai(),
        "    pub fn new() -> Self { Self { start: Instant::now() } }".ai(),
        "    pub fn elapsed(&self) -> Duration { self.start.elapsed() }".ai(),
        "    pub fn elapsed_ms(&self) -> u128 { self.elapsed().as_millis() }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Config impl + mod_b")
        .unwrap();

    // C3: append Pool impl to lib.rs + create mod_c.rs
    lib.set_contents(crate::lines![
        "pub mod types;",
        "".ai(),
        "pub struct Cache {".ai(),
        "    inner: std::collections::HashMap<String, Vec<u8>>,".ai(),
        "}".ai(),
        "impl Cache {".ai(),
        "    pub fn new() -> Self { Self { inner: Default::default() } }".ai(),
        "    pub fn get(&self, key: &str) -> Option<&Vec<u8>> { self.inner.get(key) }".ai(),
        "    pub fn set(&mut self, key: impl Into<String>, val: Vec<u8>) { self.inner.insert(key.into(), val); }".ai(),
        "}".ai(),
        "".ai(),
        "pub struct Config {".ai(),
        "    pub max_connections: usize,".ai(),
        "    pub timeout_ms: u64,".ai(),
        "}".ai(),
        "impl Default for Config {".ai(),
        "    fn default() -> Self { Self { max_connections: 10, timeout_ms: 5000 } }".ai(),
        "}".ai(),
        "impl Config {".ai(),
        "    pub fn with_timeout(mut self, ms: u64) -> Self { self.timeout_ms = ms; self }".ai(),
        "}".ai(),
        "".ai(),
        "pub struct Pool<T> { items: Vec<T> }".ai(),
        "impl<T> Pool<T> {".ai(),
        "    pub fn new(items: Vec<T>) -> Self { Self { items } }".ai(),
        "    pub fn take(&mut self) -> Option<T> { self.items.pop() }".ai(),
        "    pub fn put(&mut self, item: T) { self.items.push(item); }".ai(),
        "    pub fn len(&self) -> usize { self.items.len() }".ai(),
        "}".ai(),
    ]);
    let mut mod_c = repo.filename("src/mod_c.rs");
    mod_c.set_contents(crate::lines![
        "pub fn retry<T, E, F: Fn() -> Result<T, E>>(f: F, attempts: usize) -> Result<T, E> {".ai(),
        "    let mut last = f();".ai(),
        "    for _ in 1..attempts {".ai(),
        "        if last.is_ok() { return last; }".ai(),
        "        last = f();".ai(),
        "    }".ai(),
        "    last".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Pool impl + mod_c")
        .unwrap();

    // C4: append Event impl to lib.rs + create mod_d.rs
    lib.set_contents(crate::lines![
        "pub mod types;",
        "".ai(),
        "pub struct Cache {".ai(),
        "    inner: std::collections::HashMap<String, Vec<u8>>,".ai(),
        "}".ai(),
        "impl Cache {".ai(),
        "    pub fn new() -> Self { Self { inner: Default::default() } }".ai(),
        "    pub fn get(&self, key: &str) -> Option<&Vec<u8>> { self.inner.get(key) }".ai(),
        "    pub fn set(&mut self, key: impl Into<String>, val: Vec<u8>) { self.inner.insert(key.into(), val); }".ai(),
        "}".ai(),
        "".ai(),
        "pub struct Config { pub max_connections: usize, pub timeout_ms: u64 }".ai(),
        "impl Default for Config { fn default() -> Self { Self { max_connections: 10, timeout_ms: 5000 } } }".ai(),
        "impl Config { pub fn with_timeout(mut self, ms: u64) -> Self { self.timeout_ms = ms; self } }".ai(),
        "".ai(),
        "pub struct Pool<T> { items: Vec<T> }".ai(),
        "impl<T> Pool<T> { pub fn new(items: Vec<T>) -> Self { Self { items } } pub fn len(&self) -> usize { self.items.len() } }".ai(),
        "".ai(),
        "pub enum Event { Start, Stop, Pause, Resume }".ai(),
        "impl std::fmt::Display for Event {".ai(),
        "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {".ai(),
        "        match self { Event::Start => write!(f, \"start\"), Event::Stop => write!(f, \"stop\"),".ai(),
        "            Event::Pause => write!(f, \"pause\"), Event::Resume => write!(f, \"resume\") }".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    let mut mod_d = repo.filename("src/mod_d.rs");
    mod_d.set_contents(crate::lines![
        "pub trait Serialize { fn serialize(&self) -> Vec<u8>; }".ai(),
        "pub trait Deserialize: Sized { fn deserialize(bytes: &[u8]) -> Option<Self>; }".ai(),
        "pub fn round_trip<T: Serialize + Deserialize>(val: &T) -> Option<T> {".ai(),
        "    let bytes = val.serialize();".ai(),
        "    T::deserialize(&bytes)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Event impl + mod_d")
        .unwrap();

    // C5: append Metrics impl to lib.rs + create mod_e.rs
    lib.set_contents(crate::lines![
        "pub mod types;",
        "".ai(),
        "pub struct Cache { inner: std::collections::HashMap<String, Vec<u8>> }".ai(),
        "impl Cache { pub fn new() -> Self { Self { inner: Default::default() } } }".ai(),
        "".ai(),
        "pub struct Config { pub max_connections: usize, pub timeout_ms: u64 }".ai(),
        "impl Default for Config { fn default() -> Self { Self { max_connections: 10, timeout_ms: 5000 } } }".ai(),
        "".ai(),
        "pub struct Pool<T> { items: Vec<T> }".ai(),
        "impl<T> Pool<T> { pub fn new(items: Vec<T>) -> Self { Self { items } } pub fn len(&self) -> usize { self.items.len() } }".ai(),
        "".ai(),
        "pub enum Event { Start, Stop, Pause, Resume }".ai(),
        "impl std::fmt::Display for Event { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, \"{:?}\", self) } }".ai(),
        "".ai(),
        "pub struct Metrics { counters: std::collections::HashMap<String, u64> }".ai(),
        "impl Metrics {".ai(),
        "    pub fn new() -> Self { Self { counters: Default::default() } }".ai(),
        "    pub fn inc(&mut self, name: &str) { *self.counters.entry(name.to_owned()).or_default() += 1; }".ai(),
        "    pub fn get(&self, name: &str) -> u64 { *self.counters.get(name).unwrap_or(&0) }".ai(),
        "}".ai(),
    ]);
    let mut mod_e = repo.filename("src/mod_e.rs");
    mod_e.set_contents(crate::lines![
        "pub fn clamp<T: PartialOrd>(val: T, min: T, max: T) -> T {".ai(),
        "    if val < min { min } else if val > max { max } else { val }".ai(),
        "}".ai(),
        "pub fn lerp(a: f64, b: f64, t: f64) -> f64 { a + (b - a) * t }".ai(),
        "pub fn approx_eq(a: f64, b: f64, eps: f64) -> bool { (a - b).abs() < eps }".ai(),
        "pub fn percent(part: f64, total: f64) -> f64 { if total == 0.0 { 0.0 } else { part / total * 100.0 } }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add Metrics impl + mod_e")
        .unwrap();

    // Rebase onto main (non-conflicting: prepend + append)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {src/lib.rs, src/mod_a.rs}
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(
        &repo,
        &chain[0],
        "sha0_files",
        &["src/lib.rs", "src/mod_a.rs"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &["mod_b.rs", "mod_c.rs", "mod_d.rs", "mod_e.rs"],
    );

    // sha1 = C2': {src/lib.rs, src/mod_b.rs}
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["src/lib.rs", "src/mod_b.rs"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["mod_c.rs", "mod_d.rs", "mod_e.rs"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/lib.rs",
        "sha1_blame_new",
        &[
            ("pub struct Config {", true),
            ("impl Default for Config", true),
            ("impl Config {", true),
        ],
    );
    // mod_a.rs (from C1) is a prior file at chain[1] — fast path, verify attribution intact
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/mod_a.rs",
        "chain1_prior_mod_a_rs",
        &[
            ("pub fn encode_base64(input: &[u8]) -> String {", true),
            (
                "let alphabet = b\"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/\";",
                true,
            ),
        ],
    );

    // sha2 = C3': {src/lib.rs, src/mod_c.rs}
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["src/lib.rs", "src/mod_c.rs"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["mod_d.rs", "mod_e.rs"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/lib.rs",
        "sha2_blame_new",
        &[("pub struct Pool<T>", true), ("impl<T> Pool<T>", true)],
    );
    // mod_a.rs and mod_b.rs (from C1-C2) are prior files at chain[2]
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/mod_a.rs",
        "chain2_prior_mod_a_rs",
        &[
            ("pub fn encode_base64(input: &[u8]) -> String {", true),
            (
                "let alphabet = b\"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/\";",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/mod_b.rs",
        "chain2_prior_mod_b_rs",
        &[
            ("pub struct Timer { start: Instant }", true),
            (
                "pub fn elapsed_ms(&self) -> u128 { self.elapsed().as_millis() }",
                true,
            ),
        ],
    );

    // sha3 = C4': {src/lib.rs, src/mod_d.rs}
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(
        &repo,
        &chain[3],
        "sha3_files",
        &["src/lib.rs", "src/mod_d.rs"],
    );
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["mod_e.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/lib.rs",
        "sha3_blame_new",
        &[
            ("pub enum Event", true),
            ("impl std::fmt::Display for Event", true),
        ],
    );
    // mod_a.rs, mod_b.rs, and mod_c.rs (from C1-C3) are prior files at chain[3]
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/mod_a.rs",
        "chain3_prior_mod_a_rs",
        &[
            ("pub fn encode_base64(input: &[u8]) -> String {", true),
            (
                "let alphabet = b\"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/\";",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/mod_b.rs",
        "chain3_prior_mod_b_rs",
        &[
            ("pub struct Timer { start: Instant }", true),
            (
                "pub fn elapsed_ms(&self) -> u128 { self.elapsed().as_millis() }",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/mod_c.rs",
        "chain3_prior_mod_c_rs",
        &[
            (
                "pub fn retry<T, E, F: Fn() -> Result<T, E>>(f: F, attempts: usize) -> Result<T, E> {",
                true,
            ),
            ("if last.is_ok() { return last; }", true),
        ],
    );

    // sha4 = C5': {src/lib.rs, src/mod_e.rs}
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["src/lib.rs", "src/mod_e.rs"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/lib.rs",
        "sha4_blame_new",
        &[("pub struct Metrics {", true), ("impl Metrics {", true)],
    );
    // mod_a.rs, mod_b.rs, mod_c.rs, and mod_d.rs (from C1-C4) are prior files at chain[4]
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/mod_a.rs",
        "chain4_prior_mod_a_rs",
        &[
            ("pub fn encode_base64(input: &[u8]) -> String {", true),
            (
                "let alphabet = b\"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/\";",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/mod_b.rs",
        "chain4_prior_mod_b_rs",
        &[
            ("pub struct Timer { start: Instant }", true),
            (
                "pub fn elapsed_ms(&self) -> u128 { self.elapsed().as_millis() }",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/mod_c.rs",
        "chain4_prior_mod_c_rs",
        &[
            (
                "pub fn retry<T, E, F: Fn() -> Result<T, E>>(f: F, attempts: usize) -> Result<T, E> {",
                true,
            ),
            ("if last.is_ok() { return last; }", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/mod_d.rs",
        "chain4_prior_mod_d_rs",
        &[
            (
                "pub trait Serialize { fn serialize(&self) -> Vec<u8>; }",
                true,
            ),
            (
                "pub fn round_trip<T: Serialize + Deserialize>(val: &T) -> Option<T> {",
                true,
            ),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_rust_lib_rs_main_prepends_feature_adds_impls,);
