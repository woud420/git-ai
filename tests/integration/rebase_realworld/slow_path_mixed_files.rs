use super::*;

/// Test 7: Mixed — core.rs is shared (slow path), plus unique files in C2 and C4.
/// Critical: no future unique files leak into earlier notes.
#[test]
fn test_slow_path_mixed_unique_and_shared_files() {
    let repo = TestRepo::new();

    // Initial: core.rs with trailing newline
    repo.commit_untracked_file(
        "core.rs",
        "// Core module\npub fn init() {}\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend module-level docs to core.rs (forces slow path)
    repo.commit_untracked_file("core.rs",
        "//! Core module\n//! Provides fundamental functionality.\n\n// Core module\npub fn init() {}\n",
        "main: prepend module docs to core.rs",
    );
    repo.commit_untracked_file("lib.rs", "pub mod core;\n", "main: add lib.rs");
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"myapp\"\nversion = \"0.1.0\"\n",
        "main: add Cargo.toml",
    );
    repo.commit_untracked_file("benches/bench.rs", "fn main() {}\n", "main: add bench stub");
    repo.commit_untracked_file(
        "examples/usage.rs",
        "fn main() { println!(\"example\"); }\n",
        "main: add usage example",
    );

    // Feature from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 8 AI lines to core.rs only (no unique file)
    let mut core = repo.filename("core.rs");
    core.set_contents(crate::lines![
        "// Core module",
        "pub fn init() {}",
        "".ai(),
        "pub struct Context {".ai(),
        "    pub debug: bool,".ai(),
        "    pub log_level: u8,".ai(),
        "}".ai(),
        "impl Context {".ai(),
        "    pub fn new() -> Self { Self { debug: false, log_level: 2 } }".ai(),
        "    pub fn with_debug(mut self) -> Self { self.debug = true; self }".ai(),
        "    pub fn log(&self, msg: &str) { if self.debug { eprintln!(\"[debug] {}\", msg); } }"
            .ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add Context struct to core.rs")
        .unwrap();

    // C2: append 6 AI lines to core.rs + create module_b.rs (6 AI lines)
    core.set_contents(crate::lines![
        "// Core module",
        "pub fn init() {}",
        "".ai(),
        "pub struct Context { pub debug: bool, pub log_level: u8 }".ai(),
        "impl Context { pub fn new() -> Self { Self { debug: false, log_level: 2 } } }".ai(),
        "".ai(),
        "pub struct Registry { map: std::collections::HashMap<String, Box<dyn std::any::Any>> }".ai(),
        "impl Registry {".ai(),
        "    pub fn new() -> Self { Self { map: Default::default() } }".ai(),
        "    pub fn register<T: 'static>(&mut self, key: impl Into<String>, val: T) { self.map.insert(key.into(), Box::new(val)); }".ai(),
        "    pub fn has(&self, key: &str) -> bool { self.map.contains_key(key) }".ai(),
        "}".ai(),
    ]);
    let mut mod_b = repo.filename("module_b.rs");
    mod_b.set_contents(crate::lines![
        "pub fn hash_fnv1a(input: &[u8]) -> u64 {".ai(),
        "    let mut hash: u64 = 14695981039346656037;".ai(),
        "    for &byte in input {".ai(),
        "        hash ^= byte as u64;".ai(),
        "        hash = hash.wrapping_mul(1099511628211);".ai(),
        "    }".ai(),
        "    hash".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Registry to core.rs + module_b.rs")
        .unwrap();

    // C3: append 6 AI lines to core.rs only (no unique file)
    core.set_contents(crate::lines![
        "// Core module",
        "pub fn init() {}",
        "".ai(),
        "pub struct Context { pub debug: bool, pub log_level: u8 }".ai(),
        "impl Context { pub fn new() -> Self { Self { debug: false, log_level: 2 } } }".ai(),
        "".ai(),
        "pub struct Registry { map: std::collections::HashMap<String, Box<dyn std::any::Any>> }".ai(),
        "impl Registry { pub fn new() -> Self { Self { map: Default::default() } } pub fn has(&self, key: &str) -> bool { self.map.contains_key(key) } }".ai(),
        "".ai(),
        "pub struct EventBus { handlers: Vec<Box<dyn Fn(&str) + Send>> }".ai(),
        "impl EventBus {".ai(),
        "    pub fn new() -> Self { Self { handlers: Vec::new() } }".ai(),
        "    pub fn on<F: Fn(&str) + Send + 'static>(&mut self, f: F) { self.handlers.push(Box::new(f)); }".ai(),
        "    pub fn emit(&self, event: &str) { for h in &self.handlers { h(event); } }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add EventBus to core.rs")
        .unwrap();

    // C4: append 6 AI lines to core.rs + create module_d.rs (6 AI lines)
    core.set_contents(crate::lines![
        "// Core module",
        "pub fn init() {}",
        "".ai(),
        "pub struct Context { pub debug: bool, pub log_level: u8 }".ai(),
        "pub struct Registry { map: std::collections::HashMap<String, Box<dyn std::any::Any>> }".ai(),
        "pub struct EventBus { handlers: Vec<Box<dyn Fn(&str) + Send>> }".ai(),
        "".ai(),
        "pub struct Pipeline<T> { stages: Vec<Box<dyn Fn(T) -> T>> }".ai(),
        "impl<T: 'static> Pipeline<T> {".ai(),
        "    pub fn new() -> Self { Self { stages: Vec::new() } }".ai(),
        "    pub fn add<F: Fn(T) -> T + 'static>(&mut self, f: F) { self.stages.push(Box::new(f)); }".ai(),
        "    pub fn run(&self, input: T) -> T { self.stages.iter().fold(input, |acc, f| f(acc)) }".ai(),
        "}".ai(),
    ]);
    let mut mod_d = repo.filename("module_d.rs");
    mod_d.set_contents(crate::lines![
        "pub struct LruCache<K, V> { cap: usize, data: std::collections::HashMap<K, V> }".ai(),
        "impl<K: Eq + std::hash::Hash, V> LruCache<K, V> {".ai(),
        "    pub fn new(cap: usize) -> Self { Self { cap, data: Default::default() } }".ai(),
        "    pub fn get(&self, key: &K) -> Option<&V> { self.data.get(key) }".ai(),
        "    pub fn put(&mut self, key: K, val: V) { if self.data.len() >= self.cap { return; } self.data.insert(key, val); }".ai(),
        "    pub fn len(&self) -> usize { self.data.len() }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Pipeline to core.rs + module_d.rs")
        .unwrap();

    // C5: append 6 AI lines to core.rs only
    core.set_contents(crate::lines![
        "// Core module",
        "pub fn init() {}",
        "".ai(),
        "pub struct Context { pub debug: bool, pub log_level: u8 }".ai(),
        "pub struct Registry { map: std::collections::HashMap<String, Box<dyn std::any::Any>> }".ai(),
        "pub struct EventBus { handlers: Vec<Box<dyn Fn(&str) + Send>> }".ai(),
        "pub struct Pipeline<T> { stages: Vec<Box<dyn Fn(T) -> T>> }".ai(),
        "".ai(),
        "pub struct ServiceLocator { services: std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any>> }".ai(),
        "impl ServiceLocator {".ai(),
        "    pub fn new() -> Self { Self { services: Default::default() } }".ai(),
        "    pub fn register<T: 'static>(&mut self, service: T) { self.services.insert(std::any::TypeId::of::<T>(), Box::new(service)); }".ai(),
        "    pub fn resolve<T: 'static>(&self) -> Option<&T> { self.services.get(&std::any::TypeId::of::<T>()).and_then(|b| b.downcast_ref()) }".ai(),
        "    pub fn is_registered<T: 'static>(&self) -> bool { self.services.contains_key(&std::any::TypeId::of::<T>()) }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add ServiceLocator to core.rs")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {core.rs} only, no module_b or module_d
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["core.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &["module_b.rs", "module_d.rs"],
    );

    // sha1 = C2': {core.rs, module_b.rs}, no module_d
    // C2 added Registry struct to core.rs
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["core.rs", "module_b.rs"]);
    assert_note_no_forbidden_files(&repo, &chain[1], "sha1_no_future", &["module_d.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "core.rs",
        "sha1_core_registry",
        &[("pub struct Registry", true), ("pub fn register", true)],
    );

    // sha2 = C3': {core.rs} — C3 only changes core.rs
    // C3 added EventBus to core.rs
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["core.rs"]);
    assert_note_no_forbidden_files(&repo, &chain[2], "sha2_no_future", &["module_d.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "core.rs",
        "sha2_core_eventbus",
        &[("pub struct EventBus", true), ("pub fn emit", true)],
    );
    // module_b.rs (from C2) is a prior file at chain[2] — fast path, verify attribution intact
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "module_b.rs",
        "chain2_prior_module_b_rs",
        &[
            ("pub fn hash_fnv1a(input: &[u8]) -> u64 {", true),
            ("hash = hash.wrapping_mul(1099511628211);", true),
        ],
    );

    // sha3 = C4': {core.rs, module_d.rs}
    // C4 added Pipeline to core.rs + created module_d.rs
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["core.rs", "module_d.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "core.rs",
        "sha3_core_pipeline",
        &[("pub struct Pipeline", true), ("pub fn run", true)],
    );
    // module_b.rs (from C2) is a prior file at chain[3]
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "module_b.rs",
        "chain3_prior_module_b_rs",
        &[
            ("pub fn hash_fnv1a(input: &[u8]) -> u64 {", true),
            ("hash = hash.wrapping_mul(1099511628211);", true),
        ],
    );

    // sha4 = C5': {core.rs}
    // C5 added ServiceLocator to core.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["core.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "core.rs",
        "sha4_core_servicelocator",
        &[
            ("pub struct ServiceLocator", true),
            ("pub fn resolve", true),
        ],
    );
    // module_b.rs (from C2) and module_d.rs (from C4) are prior files at chain[4]
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "module_b.rs",
        "chain4_prior_module_b_rs",
        &[
            ("pub fn hash_fnv1a(input: &[u8]) -> u64 {", true),
            ("hash = hash.wrapping_mul(1099511628211);", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "module_d.rs",
        "chain4_prior_module_d_rs",
        &[
            (
                "pub struct LruCache<K, V> { cap: usize, data: std::collections::HashMap<K, V> }",
                true,
            ),
            (
                "pub fn put(&mut self, key: K, val: V) { if self.data.len() >= self.cap { return; } self.data.insert(key, val); }",
                true,
            ),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_mixed_unique_and_shared_files,);
