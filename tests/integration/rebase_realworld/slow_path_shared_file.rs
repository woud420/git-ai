use super::*;

/// Test 1: Python utils.py — upstream prepends module header, feature appends
/// validation/sanitization functions. Forces slow path because utils.py blobs
/// differ after rebase (upstream prepended 3 lines, feature appended AI lines).
///
/// Checks that accepted_lines at sha0 is ~8 (not the full-chain ~40), and that
/// no future commit's AI lines appear in earlier notes.
#[test]
fn test_slow_path_python_utils_main_prepends_feature_appends() {
    let repo = TestRepo::new();

    // Initial: utils.py with trailing newline so 3-way merge works cleanly.
    repo.commit_untracked_file("utils.py", "def base_util(): pass\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: prepend module header (changes blob → forces slow path on feature commits)
    repo.commit_untracked_file(
        "utils.py",
        "# utils module\nimport logging\n\ndef base_util(): pass\n",
        "main: prepend module header to utils.py",
    );
    // 4 more human commits on different files
    repo.commit_untracked_file(
        "constants.py",
        "MAX_RETRIES = 3\nTIMEOUT = 30\n",
        "main: add constants",
    );
    repo.commit_untracked_file(
        "exceptions.py",
        "class AppError(Exception): pass\nclass ValidationError(AppError): pass\n",
        "main: add exceptions",
    );
    repo.commit_untracked_file(
        "config.py",
        "import os\nDATABASE_URL = os.getenv('DATABASE_URL', 'sqlite:///app.db')\n",
        "main: add config",
    );
    repo.commit_untracked_file(
        "setup.cfg",
        "[metadata]\nname = myapp\nversion = 0.1\n",
        "main: add setup.cfg",
    );

    // Feature branch starts from BEFORE main's prepend (base = initial commit)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 8 AI lines (validate_email + sanitize_input) to utils.py
    let mut utils = repo.filename("utils.py");
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add validate_email and sanitize_input")
        .unwrap();

    // C2: append 8 more AI lines (normalize + truncate)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add normalize_phone and truncate_text")
        .unwrap();

    // C3: append 8 more AI lines (parse_date + format_currency)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
        "def parse_date(date_str: str, fmt: str = '%Y-%m-%d'):".ai(),
        "    from datetime import datetime".ai(),
        "    return datetime.strptime(date_str, fmt)".ai(),
        "".ai(),
        "def format_currency(amount: float, symbol: str = '$') -> str:".ai(),
        "    return f'{symbol}{amount:,.2f}'".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add parse_date and format_currency")
        .unwrap();

    // C4: append 8 more AI lines (generate_slug + deep_merge)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
        "def parse_date(date_str: str, fmt: str = '%Y-%m-%d'):".ai(),
        "    from datetime import datetime".ai(),
        "    return datetime.strptime(date_str, fmt)".ai(),
        "".ai(),
        "def format_currency(amount: float, symbol: str = '$') -> str:".ai(),
        "    return f'{symbol}{amount:,.2f}'".ai(),
        "".ai(),
        "def generate_slug(text: str) -> str:".ai(),
        "    import re".ai(),
        "    return re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')".ai(),
        "".ai(),
        "def deep_merge(base: dict, override: dict) -> dict:".ai(),
        "    result = dict(base)".ai(),
        "    for k, v in override.items():".ai(),
        "        result[k] = deep_merge(base[k], v) if isinstance(v, dict) and isinstance(base.get(k), dict) else v".ai(),
        "    return result".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add generate_slug and deep_merge")
        .unwrap();

    // C5: append 8 more AI lines (retry_with_backoff + chunk_list)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
        "def parse_date(date_str: str, fmt: str = '%Y-%m-%d'):".ai(),
        "    from datetime import datetime".ai(),
        "    return datetime.strptime(date_str, fmt)".ai(),
        "".ai(),
        "def format_currency(amount: float, symbol: str = '$') -> str:".ai(),
        "    return f'{symbol}{amount:,.2f}'".ai(),
        "".ai(),
        "def generate_slug(text: str) -> str:".ai(),
        "    import re".ai(),
        "    return re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')".ai(),
        "".ai(),
        "def deep_merge(base: dict, override: dict) -> dict:".ai(),
        "    result = dict(base)".ai(),
        "    for k, v in override.items():".ai(),
        "        result[k] = deep_merge(base[k], v) if isinstance(v, dict) and isinstance(base.get(k), dict) else v".ai(),
        "    return result".ai(),
        "".ai(),
        "def retry_with_backoff(fn, attempts: int = 3, base_delay: float = 0.5):".ai(),
        "    import time".ai(),
        "    for i in range(attempts):".ai(),
        "        try: return fn()".ai(),
        "        except Exception:".ai(),
        "            if i == attempts - 1: raise".ai(),
        "            time.sleep(base_delay * (2 ** i))".ai(),
        "".ai(),
        "def chunk_list(lst: list, size: int) -> list:".ai(),
        "    return [lst[i:i+size] for i in range(0, len(lst), size)]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add retry_with_backoff and chunk_list")
        .unwrap();

    // Rebase feature onto main (non-conflicting: prepend + append)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': note has utils.py only
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["utils.py"]);

    // sha0 blame: first 3 lines human (# utils module, import logging, blank),
    // then def base_util (human), then 8 AI lines
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "utils.py",
        "sha0_blame",
        &[
            ("# utils module", false),
            ("import logging", false),
            ("", false),
            ("def base_util(): pass", false),
            ("", true),
            ("def validate_email(email: str) -> bool:", true),
            ("import re", true),
            ("pattern = r'^", true),
            ("return bool(re.match(pattern, email))", true),
            ("", true),
            ("def sanitize_input(text: str) -> str:", true),
            ("return text.strip()", true),
        ],
    );

    // sha1 = C2'
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "utils.py",
        "sha1_blame_new",
        &[("def normalize_phone", true), ("def truncate_text", true)],
    );

    // sha2 = C3'
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "utils.py",
        "sha2_blame_new",
        &[("def parse_date", true), ("def format_currency", true)],
    );

    // sha3 = C4'
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "utils.py",
        "sha3_blame_new",
        &[("def generate_slug", true), ("def deep_merge", true)],
    );

    // sha4 = C5'
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "utils.py",
        "sha4_blame_new",
        &[("def retry_with_backoff", true), ("def chunk_list", true)],
    );
}

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

/// Test 3: TypeScript routes.ts — upstream prepends a comment, feature appends
/// endpoint handler functions. Blame at sha0 checks human lines at top.
#[test]
fn test_slow_path_typescript_routes_main_prepends_feature_adds_handlers() {
    let repo = TestRepo::new();

    // Initial: src/routes.ts with trailing newline
    repo.commit_untracked_file(
        "src/routes.ts",
        "import express from 'express';\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend auto-generated comment (forces slow path)
    repo.commit_untracked_file(
        "src/routes.ts",
        "// Auto-generated routes\nimport express from 'express';\n",
        "main: prepend auto-generated comment",
    );
    repo.commit_untracked_file("src/middleware.ts",
        "export const logger = (req: any, res: any, next: any) => { console.log(req.method, req.path); next(); };\n",
        "main: add logger middleware",
    );
    repo.commit_untracked_file("src/types.ts",
        "export interface User { id: number; email: string; name: string; }\nexport interface ApiResponse<T> { data: T; status: number; }\n",
        "main: add shared types",
    );
    repo.commit_untracked_file("tsconfig.json",
        "{\"compilerOptions\":{\"target\":\"ES2020\",\"module\":\"commonjs\",\"strict\":true,\"outDir\":\"dist\"},\"include\":[\"src\"]}\n",
        "main: add tsconfig",
    );
    repo.commit_untracked_file("package.json",
        "{\"name\":\"api\",\"version\":\"1.0.0\",\"scripts\":{\"build\":\"tsc\",\"start\":\"node dist/index.js\"}}\n",
        "main: add package.json",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append /users GET handler (8 AI lines)
    let mut routes = repo.filename("src/routes.ts");
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => {".ai(),
        "  try {".ai(),
        "    const users = await UserService.findAll();".ai(),
        "    res.json({ data: users, status: 200 });".ai(),
        "  } catch (err) {".ai(),
        "    res.status(500).json({ error: String(err) });".ai(),
        "  }".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add GET /users route")
        .unwrap();

    // C2: append /users POST handler (8 AI lines)
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => {".ai(),
        "  try {".ai(),
        "    const users = await UserService.findAll();".ai(),
        "    res.json({ data: users, status: 200 });".ai(),
        "  } catch (err) {".ai(),
        "    res.status(500).json({ error: String(err) });".ai(),
        "  }".ai(),
        "});".ai(),
        "".ai(),
        "router.post('/users', async (req, res) => {".ai(),
        "  const { email, name } = req.body;".ai(),
        "  if (!email || !name) return res.status(400).json({ error: 'email and name required' });"
            .ai(),
        "  const user = await UserService.create({ email, name });".ai(),
        "  res.status(201).json({ data: user, status: 201 });".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add POST /users route")
        .unwrap();

    // C3: append /users/:id GET handler (8 AI lines)
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => {".ai(),
        "  const users = await UserService.findAll();".ai(),
        "  res.json({ data: users, status: 200 });".ai(),
        "});".ai(),
        "".ai(),
        "router.post('/users', async (req, res) => {".ai(),
        "  const { email, name } = req.body;".ai(),
        "  const user = await UserService.create({ email, name });".ai(),
        "  res.status(201).json({ data: user, status: 201 });".ai(),
        "});".ai(),
        "".ai(),
        "router.get('/users/:id', async (req, res) => {".ai(),
        "  const id = parseInt(req.params.id, 10);".ai(),
        "  const user = await UserService.findById(id);".ai(),
        "  if (!user) return res.status(404).json({ error: 'Not found' });".ai(),
        "  res.json({ data: user, status: 200 });".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add GET /users/:id route")
        .unwrap();

    // C4: append /users/:id PUT handler (8 AI lines)
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => { const users = await UserService.findAll(); res.json({ data: users }); });".ai(),
        "router.post('/users', async (req, res) => { const user = await UserService.create(req.body); res.status(201).json({ data: user }); });".ai(),
        "router.get('/users/:id', async (req, res) => { const user = await UserService.findById(+req.params.id); res.json({ data: user }); });".ai(),
        "".ai(),
        "router.put('/users/:id', async (req, res) => {".ai(),
        "  const id = parseInt(req.params.id, 10);".ai(),
        "  const updates = req.body;".ai(),
        "  const user = await UserService.update(id, updates);".ai(),
        "  if (!user) return res.status(404).json({ error: 'Not found' });".ai(),
        "  res.json({ data: user, status: 200 });".ai(),
        "});".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add PUT /users/:id route")
        .unwrap();

    // C5: append /users/:id DELETE handler (8 AI lines) + export
    routes.set_contents(crate::lines![
        "import express from 'express';",
        "".ai(),
        "const router = express.Router();".ai(),
        "".ai(),
        "router.get('/users', async (req, res) => { const users = await UserService.findAll(); res.json({ data: users }); });".ai(),
        "router.post('/users', async (req, res) => { const user = await UserService.create(req.body); res.status(201).json({ data: user }); });".ai(),
        "router.get('/users/:id', async (req, res) => { const user = await UserService.findById(+req.params.id); res.json({ data: user }); });".ai(),
        "router.put('/users/:id', async (req, res) => { const user = await UserService.update(+req.params.id, req.body); res.json({ data: user }); });".ai(),
        "".ai(),
        "router.delete('/users/:id', async (req, res) => {".ai(),
        "  const id = parseInt(req.params.id, 10);".ai(),
        "  const deleted = await UserService.delete(id);".ai(),
        "  if (!deleted) return res.status(404).json({ error: 'Not found' });".ai(),
        "  res.status(204).send();".ai(),
        "});".ai(),
        "".ai(),
        "export default router;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add DELETE /users/:id + export")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': src/routes.ts with ~8 AI lines
    // blame: line 1 = human (// Auto-generated routes), line 2 = human (import express),
    // then AI lines start
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[0],
        "src/routes.ts",
        "sha0_blame",
        &[
            ("// Auto-generated routes", false),
            ("import express from 'express';", false),
            ("const router = express.Router();", true),
            ("router.get('/users'", true),
            ("try {", true),
            ("const users = await UserService.findAll()", true),
        ],
    );

    // sha1 = C2': only C2's delta
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/routes.ts",
        "sha1_blame_new",
        &[
            ("router.post('/users'", true),
            ("email and name required", true),
        ],
    );

    // sha2 = C3': only C3's delta
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/routes.ts",
        "sha2_blame_new",
        &[
            ("router.get('/users/:id'", true),
            ("UserService.findById", true),
        ],
    );

    // sha3 = C4': only C4's delta
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/routes.ts",
        "sha3_blame_new",
        &[
            ("router.put('/users/:id'", true),
            ("UserService.update", true),
        ],
    );

    // sha4 = C5': only C5's delta
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["src/routes.ts"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/routes.ts",
        "sha4_blame_new",
        &[
            ("router.delete('/users/:id'", true),
            ("export default router", true),
        ],
    );
}

/// Test 4: TOML config file — upstream prepends production header, feature
/// appends new TOML sections per commit. Each commit adds 8+ AI lines.
/// Verifies no future sections bleed into earlier commit notes.
#[test]
fn test_slow_path_config_file_both_add_different_sections() {
    let repo = TestRepo::new();

    // Initial: config.toml with trailing newline
    repo.commit_untracked_file(
        "config.toml",
        "[server]\nhost = \"localhost\"\nport = 8080\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend production comment (forces slow path on feature commits)
    repo.commit_untracked_file(
        "config.toml",
        "# Production config\n\n[server]\nhost = \"localhost\"\nport = 8080\n",
        "main: prepend production config header",
    );
    repo.commit_untracked_file(
        ".env.production",
        "APP_ENV=production\nLOG_LEVEL=warn\n",
        "main: add production env",
    );
    repo.commit_untracked_file(
        "docker-compose.prod.yml",
        "version: '3.9'\nservices:\n  app:\n    image: myapp:latest\n    ports: ['80:8080']\n",
        "main: add prod docker-compose",
    );
    repo.commit_untracked_file(
        "nginx.conf",
        "server { listen 80; location / { proxy_pass http://app:8080; } }\n",
        "main: add nginx config",
    );
    repo.commit_untracked_file(
        "Makefile",
        "deploy:\n\tdocker-compose -f docker-compose.prod.yml up -d\n",
        "main: add Makefile",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append [database] section (8 AI lines)
    let mut cfg = repo.filename("config.toml");
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "min_connections = 5".ai(),
        "connect_timeout = 30".ai(),
        "idle_timeout = 600".ai(),
        "max_lifetime = 1800".ai(),
        "ssl_mode = \"require\"".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add [database] config section")
        .unwrap();

    // C2: append [cache] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "min_connections = 5".ai(),
        "connect_timeout = 30".ai(),
        "idle_timeout = 600".ai(),
        "max_lifetime = 1800".ai(),
        "ssl_mode = \"require\"".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "url = \"redis://localhost:6379/0\"".ai(),
        "max_size = 1000".ai(),
        "ttl_seconds = 300".ai(),
        "eviction_policy = \"lru\"".ai(),
        "compression = true".ai(),
        "key_prefix = \"app:\"".ai(),
        "serializer = \"json\"".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add [cache] config section")
        .unwrap();

    // C3: append [metrics] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "ssl_mode = \"require\"".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "url = \"redis://localhost:6379/0\"".ai(),
        "ttl_seconds = 300".ai(),
        "".ai(),
        "[metrics]".ai(),
        "enabled = true".ai(),
        "endpoint = \"/metrics\"".ai(),
        "port = 9090".ai(),
        "interval_seconds = 15".ai(),
        "include_system = true".ai(),
        "labels = [\"app\", \"env\", \"version\"]".ai(),
        "exporter = \"prometheus\"".ai(),
        "histogram_buckets = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add [metrics] config section")
        .unwrap();

    // C4: append [auth] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "url = \"redis://localhost:6379/0\"".ai(),
        "".ai(),
        "[metrics]".ai(),
        "enabled = true".ai(),
        "endpoint = \"/metrics\"".ai(),
        "".ai(),
        "[auth]".ai(),
        "provider = \"jwt\"".ai(),
        "secret_env = \"JWT_SECRET\"".ai(),
        "token_expiry_seconds = 3600".ai(),
        "refresh_expiry_seconds = 86400".ai(),
        "algorithm = \"HS256\"".ai(),
        "issuer = \"myapp\"".ai(),
        "audience = [\"web\", \"mobile\"]".ai(),
        "allow_anonymous = false".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add [auth] config section")
        .unwrap();

    // C5: append [notifications] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "".ai(),
        "[metrics]".ai(),
        "enabled = true".ai(),
        "".ai(),
        "[auth]".ai(),
        "provider = \"jwt\"".ai(),
        "token_expiry_seconds = 3600".ai(),
        "".ai(),
        "[notifications]".ai(),
        "email_driver = \"smtp\"".ai(),
        "smtp_host = \"smtp.sendgrid.net\"".ai(),
        "smtp_port = 587".ai(),
        "smtp_user_env = \"SMTP_USER\"".ai(),
        "smtp_pass_env = \"SMTP_PASS\"".ai(),
        "from_address = \"noreply@myapp.com\"".ai(),
        "queue_name = \"notifications\"".ai(),
        "retry_attempts = 3".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add [notifications] config section")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': config.toml with [database] section only
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["config.toml"]);

    // sha1 = C2': [cache] section only
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "config.toml",
        "sha1_blame_new",
        &[
            ("[cache]", true),
            ("backend = \"redis\"", true),
            ("eviction_policy", true),
        ],
    );

    // sha2 = C3': [metrics] section only
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "config.toml",
        "sha2_blame_new",
        &[
            ("[metrics]", true),
            ("exporter = \"prometheus\"", true),
            ("histogram_buckets", true),
        ],
    );

    // sha3 = C4': [auth] section only
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "config.toml",
        "sha3_blame_new",
        &[
            ("[auth]", true),
            ("provider = \"jwt\"", true),
            ("allow_anonymous = false", true),
        ],
    );

    // sha4 = C5': [notifications] section only
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "config.toml",
        "sha4_blame_new",
        &[
            ("[notifications]", true),
            ("email_driver = \"smtp\"", true),
            ("retry_attempts = 3", true),
        ],
    );
}

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

crate::reuse_tests_in_worktree!(
    test_slow_path_python_utils_main_prepends_feature_appends,
    test_slow_path_rust_lib_rs_main_prepends_feature_adds_impls,
    test_slow_path_typescript_routes_main_prepends_feature_adds_handlers,
    test_slow_path_config_file_both_add_different_sections,
    test_slow_path_growing_shared_file_10_commits,
);
