use super::*;

/// Test 6: Two shared files (models.py + services.py), both prepended by main.
/// Feature appends AI lines to both in each commit. Checks cumulative lines
/// across both files and no future-file leak.
#[test]
fn test_slow_path_multiple_shared_files_both_modified() {
    let repo = TestRepo::new();

    // Initial: both shared files with trailing newline
    repo.commit_untracked_file(
        "models.py",
        "class BaseModel: pass\n",
        "Initial commit: models.py",
    );
    repo.commit_untracked_file(
        "services.py",
        "class BaseService: pass\n",
        "Initial commit: services.py",
    );
    let main_branch = repo.current_branch();

    // Main: prepend headers to BOTH files (two separate commits, then 3 more human commits)
    repo.commit_untracked_file(
        "models.py",
        "# Domain models\nfrom dataclasses import dataclass\n\nclass BaseModel: pass\n",
        "main: prepend header to models.py",
    );
    repo.commit_untracked_file(
        "services.py",
        "# Business services\nfrom typing import Any\n\nclass BaseService: pass\n",
        "main: prepend header to services.py",
    );
    repo.commit_untracked_file(
        "exceptions.py",
        "class NotFound(Exception): pass\nclass Conflict(Exception): pass\n",
        "main: add exceptions",
    );
    repo.commit_untracked_file("validators.py",
        "def validate_not_empty(val, name):\n    if not val: raise ValueError(f'{name} must not be empty')\n",
        "main: add validators",
    );
    repo.commit_untracked_file(
        "constants.py",
        "DEFAULT_PAGE_SIZE = 20\nMAX_PAGE_SIZE = 100\n",
        "main: add constants",
    );

    // Feature branch from before main's two prepend commits (HEAD~5)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 6 AI lines to BOTH models.py AND services.py (12 total)
    let mut models = repo.filename("models.py");
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    email: str".ai(),
        "    name: str".ai(),
        "    active: bool = True".ai(),
    ]);
    let mut services = repo.filename("services.py");
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_id(self, user_id: int): return self.repo.find(user_id)".ai(),
        "    def list_active(self): return self.repo.find_all(active=True)".ai(),
        "    def deactivate(self, user_id: int): self.repo.update(user_id, active=False)".ai(),
        "    def exists(self, email: str) -> bool: return self.repo.find_by_email(email) is not None".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add User model + UserService")
        .unwrap();

    // C2: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    email: str".ai(),
        "    name: str".ai(),
        "    active: bool = True".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Product:".ai(),
        "    id: int".ai(),
        "    name: str".ai(),
        "    price: float".ai(),
        "    stock: int = 0".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_id(self, user_id: int): return self.repo.find(user_id)".ai(),
        "    def list_active(self): return self.repo.find_all(active=True)".ai(),
        "".ai(),
        "class ProductService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_id(self, pid: int): return self.repo.find(pid)".ai(),
        "    def list_in_stock(self): return self.repo.find_all(stock__gt=0)".ai(),
        "    def adjust_stock(self, pid: int, delta: int): self.repo.increment(pid, 'stock', delta)".ai(),
        "    def get_price(self, pid: int) -> float: return self.repo.find(pid).price".ai(),
        "    def set_price(self, pid: int, price: float): self.repo.update(pid, price=price)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Product model + ProductService")
        .unwrap();

    // C3: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User: id: int; email: str; name: str; active: bool = True".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Product: id: int; name: str; price: float; stock: int = 0".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Order:".ai(),
        "    id: int".ai(),
        "    user_id: int".ai(),
        "    items: list".ai(),
        "    total: float".ai(),
        "    status: str = 'pending'".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService:".ai(),
        "    def get_by_id(self, user_id: int): return self.repo.find(user_id)".ai(),
        "".ai(),
        "class ProductService:".ai(),
        "    def get_by_id(self, pid: int): return self.repo.find(pid)".ai(),
        "    def list_in_stock(self): return self.repo.find_all(stock__gt=0)".ai(),
        "".ai(),
        "class OrderService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def create(self, user_id, items): return self.repo.create(user_id=user_id, items=items)".ai(),
        "    def get_by_id(self, oid: int): return self.repo.find(oid)".ai(),
        "    def cancel(self, oid: int): self.repo.update(oid, status='cancelled')".ai(),
        "    def complete(self, oid: int): self.repo.update(oid, status='completed')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Order model + OrderService")
        .unwrap();

    // C4: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User: id: int; email: str; name: str".ai(),
        "@dataclass".ai(),
        "class Product: id: int; name: str; price: float; stock: int = 0".ai(),
        "@dataclass".ai(),
        "class Order: id: int; user_id: int; items: list; total: float; status: str = 'pending'"
            .ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Address:".ai(),
        "    id: int".ai(),
        "    user_id: int".ai(),
        "    street: str".ai(),
        "    city: str".ai(),
        "    country: str = 'US'".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService: pass".ai(),
        "class ProductService: pass".ai(),
        "class OrderService: pass".ai(),
        "".ai(),
        "class AddressService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def get_by_user(self, uid: int): return self.repo.find_all(user_id=uid)".ai(),
        "    def create(self, uid, street, city, country='US'): return self.repo.create(user_id=uid, street=street, city=city, country=country)".ai(),
        "    def delete(self, aid: int): self.repo.delete(aid)".ai(),
        "    def set_default(self, uid: int, aid: int): self.repo.update_all({'is_default': False}, user_id=uid); self.repo.update(aid, is_default=True)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Address model + AddressService")
        .unwrap();

    // C5: append 6 more AI lines to both files
    models.set_contents(crate::lines![
        "class BaseModel: pass",
        "".ai(),
        "@dataclass".ai(),
        "class User: id: int; email: str; name: str".ai(),
        "@dataclass".ai(),
        "class Product: id: int; name: str; price: float; stock: int = 0".ai(),
        "@dataclass".ai(),
        "class Order: id: int; user_id: int; items: list; total: float; status: str = 'pending'"
            .ai(),
        "@dataclass".ai(),
        "class Address: id: int; user_id: int; street: str; city: str; country: str = 'US'".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class Review:".ai(),
        "    id: int".ai(),
        "    user_id: int".ai(),
        "    product_id: int".ai(),
        "    rating: int".ai(),
        "    comment: str = ''".ai(),
    ]);
    services.set_contents(crate::lines![
        "class BaseService: pass",
        "".ai(),
        "class UserService: pass".ai(),
        "class ProductService: pass".ai(),
        "class OrderService: pass".ai(),
        "class AddressService: pass".ai(),
        "".ai(),
        "class ReviewService:".ai(),
        "    def __init__(self, repo): self.repo = repo".ai(),
        "    def create(self, uid, pid, rating, comment=''): return self.repo.create(user_id=uid, product_id=pid, rating=rating, comment=comment)".ai(),
        "    def get_for_product(self, pid: int): return self.repo.find_all(product_id=pid)".ai(),
        "    def average_rating(self, pid: int) -> float: reviews = self.get_for_product(pid); return sum(r.rating for r in reviews) / len(reviews) if reviews else 0.0".ai(),
        "    def delete(self, rid: int): self.repo.delete(rid)".ai(),
        "    def update_comment(self, rid: int, comment: str): self.repo.update(rid, comment=comment)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add Review model + ReviewService")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {models.py, services.py} ~12 accepted lines
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(
        &repo,
        &chain[0],
        "sha0_files",
        &["models.py", "services.py"],
    );
    // sha1 = C2': {models.py, services.py} ~12 accepted lines (only C2's delta)
    // C2 added Product model to models.py and ProductService to services.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "models.py",
        "sha1_models_product",
        &[("class Product:", true), ("price: float", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "services.py",
        "sha1_services_product",
        &[("class ProductService:", true), ("def list_in_stock", true)],
    );

    // sha2 = C3': {models.py, services.py} ~12 accepted lines (only C3's delta)
    // C3 added Order model to models.py and OrderService to services.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "models.py",
        "sha2_models_order",
        &[("class Order:", true), ("status: str = 'pending'", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "services.py",
        "sha2_services_order",
        &[("class OrderService:", true), ("def cancel", true)],
    );

    // sha3 = C4': ~12 accepted lines (only C4's delta)
    // C4 added Address model to models.py and AddressService to services.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(
        &repo,
        &chain[3],
        "sha3_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "models.py",
        "sha3_models_address",
        &[("class Address:", true), ("country: str = 'US'", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "services.py",
        "sha3_services_address",
        &[("class AddressService:", true), ("def get_by_user", true)],
    );

    // sha4 = C5': ~12 accepted lines (only C5's delta)
    // C5 added Review model to models.py and ReviewService to services.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["models.py", "services.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "models.py",
        "sha4_models_review",
        &[("class Review:", true), ("rating: int", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "services.py",
        "sha4_services_review",
        &[("class ReviewService:", true), ("def average_rating", true)],
    );
}

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

/// Test 8: Feature has human commits intermixed with AI commits.
/// C1 and C4 are human-only. C2, C3, C5 append AI to api.py.
/// Checks that human-only commits don't introduce phantom attribution,
/// and cumulative AI lines are stable across the human commits.
#[test]
fn test_slow_path_feature_has_human_commits_intermixed() {
    let repo = TestRepo::new();

    // Initial: api.py with trailing newline
    repo.commit_untracked_file("api.py", "# API module\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: prepend import block to api.py (forces slow path)
    repo.commit_untracked_file(
        "api.py",
        "from flask import Flask, request, jsonify\nfrom functools import wraps\n\n# API module\n",
        "main: prepend imports to api.py",
    );
    repo.commit_untracked_file(
        "wsgi.py",
        "from api import app\nif __name__ == '__main__': app.run()\n",
        "main: add wsgi.py",
    );
    repo.commit_untracked_file(
        "gunicorn.conf.py",
        "bind = '0.0.0.0:8000'\nworkers = 4\ntimeout = 30\n",
        "main: add gunicorn config",
    );
    repo.commit_untracked_file(
        ".flake8",
        "[flake8]\nmax-line-length = 120\n",
        "main: add flake8 config",
    );
    repo.commit_untracked_file(
        "pytest.ini",
        "[pytest]\ntestpaths = tests\naddopts = -v\n",
        "main: add pytest config",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: HUMAN only — adds config.py (plain write, no AI)
    repo.commit_untracked_file("config.py",
        "import os\nDATABASE_URL = os.getenv('DATABASE_URL', 'sqlite:///app.db')\nSECRET_KEY = os.getenv('SECRET_KEY', 'dev-secret')\nDEBUG = os.getenv('DEBUG', '0') == '1'\nALLOWED_HOSTS = os.getenv('ALLOWED_HOSTS', 'localhost').split(',')\n",
        "config: add application config",
    );

    // C2: AI — appends 10 AI lines to api.py
    let mut api = repo.filename("api.py");
    api.set_contents(crate::lines![
        "# API module",
        "".ai(),
        "app = Flask(__name__)".ai(),
        "".ai(),
        "def require_auth(f):".ai(),
        "    @wraps(f)".ai(),
        "    def decorated(*args, **kwargs):".ai(),
        "        token = request.headers.get('Authorization', '').replace('Bearer ', '')".ai(),
        "        if not token: return jsonify({'error': 'unauthorized'}), 401".ai(),
        "        return f(*args, **kwargs)".ai(),
        "    return decorated".ai(),
        "".ai(),
        "@app.route('/health')".ai(),
        "def health(): return jsonify({'status': 'ok', 'version': '1.0'})".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Flask app and /health endpoint")
        .unwrap();

    // C3: AI — appends 10 more AI lines to api.py
    api.set_contents(crate::lines![
        "# API module",
        "".ai(),
        "app = Flask(__name__)".ai(),
        "".ai(),
        "def require_auth(f):".ai(),
        "    @wraps(f)".ai(),
        "    def decorated(*args, **kwargs):".ai(),
        "        token = request.headers.get('Authorization', '').replace('Bearer ', '')".ai(),
        "        if not token: return jsonify({'error': 'unauthorized'}), 401".ai(),
        "        return f(*args, **kwargs)".ai(),
        "    return decorated".ai(),
        "".ai(),
        "@app.route('/health')".ai(),
        "def health(): return jsonify({'status': 'ok', 'version': '1.0'})".ai(),
        "".ai(),
        "@app.route('/users', methods=['GET'])".ai(),
        "@require_auth".ai(),
        "def list_users():".ai(),
        "    from config import DATABASE_URL".ai(),
        "    return jsonify({'users': [], 'database': DATABASE_URL})".ai(),
        "".ai(),
        "@app.route('/users', methods=['POST'])".ai(),
        "@require_auth".ai(),
        "def create_user():".ai(),
        "    data = request.get_json()".ai(),
        "    return jsonify({'created': data}), 201".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add /users GET and POST endpoints")
        .unwrap();

    // C4: HUMAN only — adds requirements.txt (no AI)
    repo.commit_untracked_file(
        "requirements.txt",
        "flask==3.0.0\ngunicorn==21.2.0\nrequests==2.31.0\npytest==7.4.0\ncoverage==7.3.0\n",
        "deps: add requirements.txt",
    );

    // C5: AI — appends 10 more AI lines to api.py
    api.set_contents(crate::lines![
        "# API module",
        "".ai(),
        "app = Flask(__name__)".ai(),
        "".ai(),
        "def require_auth(f):".ai(),
        "    @wraps(f)".ai(),
        "    def decorated(*args, **kwargs):".ai(),
        "        token = request.headers.get('Authorization', '').replace('Bearer ', '')".ai(),
        "        if not token: return jsonify({'error': 'unauthorized'}), 401".ai(),
        "        return f(*args, **kwargs)".ai(),
        "    return decorated".ai(),
        "".ai(),
        "@app.route('/health')".ai(),
        "def health(): return jsonify({'status': 'ok'})".ai(),
        "".ai(),
        "@app.route('/users', methods=['GET'])".ai(),
        "@require_auth".ai(),
        "def list_users(): return jsonify({'users': []})".ai(),
        "".ai(),
        "@app.route('/users', methods=['POST'])".ai(),
        "@require_auth".ai(),
        "def create_user(): return jsonify({'created': request.get_json()}), 201".ai(),
        "".ai(),
        "@app.route('/users/<int:uid>', methods=['GET'])".ai(),
        "@require_auth".ai(),
        "def get_user(uid: int): return jsonify({'user': {'id': uid}})".ai(),
        "".ai(),
        "@app.route('/users/<int:uid>', methods=['DELETE'])".ai(),
        "@require_auth".ai(),
        "def delete_user(uid: int): return '', 204".ai(),
        "".ai(),
        "@app.errorhandler(404)".ai(),
        "def not_found(e): return jsonify({'error': 'not found'}), 404".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add /users/:id GET and DELETE endpoints")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);
    // chain[0]=C1'(human), chain[1]=C2'(AI), chain[2]=C3'(AI), chain[3]=C4'(human), chain[4]=C5'(AI)

    // sha0 = C1' (human-only commit: config.py via commit_untracked_file, no note expected).
    assert_note_no_forbidden_files_if_present(&repo, &chain[0], "sha0_no_api", &["api.py"]);

    // sha1 = C2' (first AI commit): api.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["api.py"]);
    // C2 introduced Flask app + /health endpoint — verify they are AI at sha1.
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "api.py",
        "sha1_blame",
        &[
            ("app = Flask(__name__)", true),
            ("def require_auth", true),
            ("def health", true),
        ],
    );

    // sha2 = C3' (second AI commit): api.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["api.py"]);
    // C3 introduced /users GET and POST routes.
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "api.py",
        "sha2_blame",
        &[("def list_users", true), ("def create_user", true)],
    );

    // sha3 = C4' (human-only commit: requirements.txt via commit_untracked_file, no note expected).
    assert_note_no_forbidden_files_if_present(
        &repo,
        &chain[3],
        "sha3_no_future",
        &["config.py", "requirements.txt"],
    );

    // sha4 = C5' (third AI commit): api.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["api.py"]);
    // C5 introduced /users/:id GET and DELETE — verify they are AI at sha4.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "api.py",
        "sha4_blame",
        &[
            ("def get_user", true),
            ("def delete_user", true),
            ("def not_found", true),
        ],
    );

    // Session format: cumulative AI lines in attestation ranges.
    // Values grow monotonically: [12, 18, 30].
}

/// Test 9: Large function blocks with 20-line license header prepended.
/// Feature adds 15-AI-line functions to processor.rs per commit.
/// Line offsets shift by 20 after rebase. Blame at sha0 checks the offset.
#[test]
fn test_slow_path_large_function_blocks_line_offset() {
    let repo = TestRepo::new();

    // Initial: processor.rs with two human lines + trailing newline
    repo.commit_untracked_file(
        "processor.rs",
        "// Processor module\nuse std::io;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend a 20-line license header (forces slow path, creates big line offset)
    repo.commit_untracked_file(
        "processor.rs",
        concat!(
            "// Copyright 2024 MyOrg. All rights reserved.\n",
            "// \n",
            "// Redistribution and use in source and binary forms,\n",
            "// with or without modification, are permitted provided\n",
            "// that the following conditions are met:\n",
            "// \n",
            "//   1. Redistributions of source code must retain the\n",
            "//      above copyright notice, this list of conditions\n",
            "//      and the following disclaimer.\n",
            "// \n",
            "//   2. Redistributions in binary form must reproduce the\n",
            "//      above copyright notice, this list of conditions\n",
            "//      and the following disclaimer in the documentation.\n",
            "// \n",
            "// THIS SOFTWARE IS PROVIDED 'AS IS' WITHOUT WARRANTY\n",
            "// OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT\n",
            "// LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS\n",
            "// FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.\n",
            "// See the License file for details.\n",
            "//\n",
            "// Processor module\n",
            "use std::io;\n",
        ),
        "main: prepend 20-line license header to processor.rs",
    );
    repo.commit_untracked_file(
        "error.rs",
        "#[derive(Debug)] pub enum ProcessError { Io(std::io::Error), Invalid(String) }\n",
        "main: add error types",
    );
    repo.commit_untracked_file("types.rs",
        "pub type Bytes = Vec<u8>;\npub type Result<T> = std::result::Result<T, crate::error::ProcessError>;\n",
        "main: add common types",
    );
    repo.commit_untracked_file(
        "tests/smoke.rs",
        "#[test] fn smoke() { assert!(true); }\n",
        "main: add smoke test",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"processor\"\nversion = \"0.1.0\"\n",
        "main: add Cargo.toml",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: add 15-AI-line function process_batch to processor.rs
    let mut proc = repo.filename("processor.rs");
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    if data.is_empty() {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty input\"));".ai(),
        "    }".ai(),
        "    let mut out = Vec::with_capacity(data.len());".ai(),
        "    for &byte in data {".ai(),
        "        let processed = if byte.is_ascii_uppercase() {".ai(),
        "            byte.to_ascii_lowercase()".ai(),
        "        } else if byte.is_ascii_lowercase() {".ai(),
        "            byte.to_ascii_uppercase()".ai(),
        "        } else {".ai(),
        "            byte".ai(),
        "        };".ai(),
        "        out.push(processed);".ai(),
        "    }".ai(),
        "    Ok(out)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add process_batch function")
        .unwrap();

    // C2: add 15-AI-line function validate_input
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    if data.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty\")); }".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.is_empty() {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"data is empty\"));".ai(),
        "    }".ai(),
        "    if data.len() > max_len {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput,".ai(),
        "            format!(\"data length {} exceeds max {}\", data.len(), max_len)));".ai(),
        "    }".ai(),
        "    if data.iter().any(|&b| b == 0) {".ai(),
        "        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"null byte\"));".ai(),
        "    }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add validate_input function")
        .unwrap();

    // C3: add 15-AI-line function chunk_data
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty\")); }".ai(),
        "    if data.len() > max_len { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"too long\")); }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn chunk_data(data: &[u8], chunk_size: usize) -> Vec<&[u8]> {".ai(),
        "    if chunk_size == 0 { return Vec::new(); }".ai(),
        "    let n = (data.len() + chunk_size - 1) / chunk_size;".ai(),
        "    let mut chunks = Vec::with_capacity(n);".ai(),
        "    let mut offset = 0;".ai(),
        "    while offset < data.len() {".ai(),
        "        let end = (offset + chunk_size).min(data.len());".ai(),
        "        chunks.push(&data[offset..end]);".ai(),
        "        offset += chunk_size;".ai(),
        "    }".ai(),
        "    chunks".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add chunk_data function")
        .unwrap();

    // C4: add 15-AI-line function compress
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.is_empty() { return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"empty\")); }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
        "pub fn chunk_data(data: &[u8], size: usize) -> Vec<&[u8]> {".ai(),
        "    if size == 0 { return Vec::new(); }".ai(),
        "    (0..data.len()).step_by(size).map(|i| &data[i..(i + size).min(data.len())]).collect()".ai(),
        "}".ai(),
        "".ai(),
        "pub fn run_length_encode(data: &[u8]) -> Vec<(u8, usize)> {".ai(),
        "    if data.is_empty() { return Vec::new(); }".ai(),
        "    let mut result = Vec::new();".ai(),
        "    let mut current = data[0];".ai(),
        "    let mut count = 1usize;".ai(),
        "    for &b in &data[1..] {".ai(),
        "        if b == current { count += 1; }".ai(),
        "        else { result.push((current, count)); current = b; count = 1; }".ai(),
        "    }".ai(),
        "    result.push((current, count));".ai(),
        "    result".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add run_length_encode function")
        .unwrap();

    // C5: add 15-AI-line function transform_pipeline
    proc.set_contents(crate::lines![
        "// Processor module",
        "use std::io;",
        "".ai(),
        "pub fn process_batch(data: &[u8]) -> Result<Vec<u8>, std::io::Error> {".ai(),
        "    Ok(data.iter().map(|&b| if b.is_ascii_alphabetic() { b ^ 0x20 } else { b }).collect())".ai(),
        "}".ai(),
        "pub fn validate_input(data: &[u8], max_len: usize) -> Result<(), std::io::Error> {".ai(),
        "    if data.len() > max_len { Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, \"too long\")) } else { Ok(()) }".ai(),
        "}".ai(),
        "pub fn chunk_data(data: &[u8], size: usize) -> Vec<&[u8]> {".ai(),
        "    (0..data.len()).step_by(size).map(|i| &data[i..(i+size).min(data.len())]).collect()".ai(),
        "}".ai(),
        "pub fn run_length_encode(data: &[u8]) -> Vec<(u8, usize)> {".ai(),
        "    let mut r = Vec::new(); let mut c = data[0]; let mut n = 1usize;".ai(),
        "    for &b in &data[1..] { if b == c { n += 1; } else { r.push((c, n)); c = b; n = 1; } }".ai(),
        "    r.push((c, n)); r".ai(),
        "}".ai(),
        "".ai(),
        "pub fn transform_pipeline(data: &[u8], transforms: &[fn(&[u8]) -> Vec<u8>]) -> Vec<u8> {".ai(),
        "    let mut current = data.to_vec();".ai(),
        "    for transform in transforms {".ai(),
        "        current = transform(&current);".ai(),
        "    }".ai(),
        "    current".ai(),
        "}".ai(),
        "".ai(),
        "pub fn hexdump(data: &[u8]) -> String {".ai(),
        "    data.iter().map(|b| format!(\"{:02x}\", b)).collect::<Vec<_>>().join(\" \")".ai(),
        "}".ai(),
        "".ai(),
        "pub fn count_bytes(data: &[u8]) -> std::collections::HashMap<u8, usize> {".ai(),
        "    let mut map = std::collections::HashMap::new();".ai(),
        "    for &b in data { *map.entry(b).or_insert(0) += 1; }".ai(),
        "    map".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add transform_pipeline and helpers")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': processor.rs
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["processor.rs"]);

    // sha0 blame: 20 license lines (human) + 1 "// Processor module" (human) + 1 "use std::io;" (human)
    // then the blank + function (AI lines) start
    assert_blame_sample_at_commit(
        &repo,
        &chain[0],
        "processor.rs",
        "sha0_blame_offset",
        &[
            ("// Copyright 2024 MyOrg", false),
            ("// Redistribution", false),
            ("// THIS SOFTWARE IS PROVIDED", false),
            ("// Processor module", false),
            ("use std::io;", false),
            ("pub fn process_batch", true),
        ],
    );

    // sha1 = C2': C2 added validate_input function.
    // The 20-line license header prepend shifts ALL feature lines by 20.
    // assert_blame_sample_at_commit verifies key lines across the intermediate commit,
    // confirming the line-offset accounting is correct after the upstream prepend.
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["processor.rs"]);
    // The 20-line license header shifts ALL feature lines by +20. We check that
    // known-AI lines in the intermediate commit C2′ are correctly attributed.
    // Only lines whose content also exists in the final feature tip (C5) are
    // attributable via the hunk-based content-map lookup; lines that C3/C4/C5
    // later rewrote are no longer in the content map and show as human.
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "processor.rs",
        "sha1_blame_offset",
        &[
            ("// Copyright 2024 MyOrg", false), // license header (human) — not AI
            ("// Processor module", false),     // original human header
            ("use std::io;", false),            // original human line
            ("pub fn process_batch", true),     // C1 AI line, offset +20 correctly applied
            ("pub fn validate_input", true),    // C2 AI line — function sig survived to tip
        ],
    );

    // sha2 = C3': C3 added chunk_data function
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "processor.rs",
        "sha2_chunk_data",
        &[
            ("pub fn chunk_data", true),
            ("chunk_size == 0", true),
            ("chunks.push", true),
        ],
    );

    // sha3 = C4': C4 added run_length_encode function
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "processor.rs",
        "sha3_rle",
        &[("pub fn run_length_encode", true), ("result.push", true)],
    );

    // sha4 = C5': C5 added transform_pipeline and helpers
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "processor.rs",
        "sha4_transform",
        &[
            ("pub fn transform_pipeline", true),
            ("pub fn hexdump", true),
            ("pub fn count_bytes", true),
        ],
    );
}

/// Test 10: Shared file grows AND each commit adds a unique helper file.
/// shared_util.js prepended by main; feature appends 8 lines to it and
/// creates helpers/X.js per commit. Checks cumulative file sets at every SHA.
#[test]
fn test_slow_path_file_grows_then_unique_files_each_commit() {
    let repo = TestRepo::new();

    // Initial: shared_util.js with trailing newline
    repo.commit_untracked_file(
        "shared_util.js",
        "export const VERSION = '1.0';\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend 'use strict' directive (forces slow path)
    repo.commit_untracked_file(
        "shared_util.js",
        "'use strict';\n\nexport const VERSION = '1.0';\n",
        "main: prepend use strict to shared_util.js",
    );
    repo.commit_untracked_file(
        "package.json",
        "{\"name\":\"helpers\",\"version\":\"1.0.0\",\"type\":\"module\"}\n",
        "main: add package.json",
    );
    repo.commit_untracked_file(
        ".eslintrc.json",
        "{\"env\":{\"es2022\":true},\"extends\":[\"eslint:recommended\"]}\n",
        "main: add eslint config",
    );
    repo.commit_untracked_file(
        "vitest.config.js",
        "export default {test:{environment:'node'}};\n",
        "main: add vitest config",
    );
    repo.commit_untracked_file(
        "README.md",
        "# Helpers\n\nA collection of JavaScript helper modules.\n",
        "main: add README",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 8 AI lines to shared_util.js + create helpers/date.js (6 AI lines)
    let mut shared = repo.filename("shared_util.js");
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export function clamp(n, min, max) { return Math.min(Math.max(n, min), max); }".ai(),
        "export function lerp(a, b, t) { return a + (b - a) * t; }".ai(),
        "export function noop() {}".ai(),
        "export const identity = x => x;".ai(),
        "export function once(fn) { let called = false, result; return (...a) => { if (!called) { called = true; result = fn(...a); } return result; }; }".ai(),
        "export function memoize(fn) { const cache = new Map(); return (...a) => { const k = JSON.stringify(a); if (!cache.has(k)) cache.set(k, fn(...a)); return cache.get(k); }; }".ai(),
        "export function pipe(...fns) { return x => fns.reduce((v, f) => f(v), x); }".ai(),
        "export function compose(...fns) { return x => fns.reduceRight((v, f) => f(v), x); }".ai(),
    ]);
    // Ensure the helpers directory exists
    let helpers_dir = repo.path().join("helpers");
    fs::create_dir_all(&helpers_dir).expect("create helpers dir");
    let mut date_helper = repo.filename("helpers/date.js");
    date_helper.set_contents(crate::lines![
        "export const now = () => new Date();".ai(),
        "export const today = () => { const d = new Date(); d.setHours(0,0,0,0); return d; };".ai(),
        "export const addDays = (d, n) => { const r = new Date(d); r.setDate(r.getDate()+n); return r; };".ai(),
        "export const formatISO = d => d.toISOString().slice(0, 10);".ai(),
        "export const isWeekend = d => d.getDay() === 0 || d.getDay() === 6;".ai(),
        "export const diffMs = (a, b) => Math.abs(new Date(a) - new Date(b));".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 grow shared_util.js + helpers/date.js")
        .unwrap();

    // C2: append 8 more AI lines to shared_util.js + create helpers/string.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const identity = x => x;".ai(),
        "export const once = fn => { let c=false,r; return (...a) => { if(!c){c=true;r=fn(...a);} return r; }; };".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); if(!m.has(k)) m.set(k,fn(...a)); return m.get(k); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "".ai(),
        "export function curry(fn) { return function curried(...args) { return args.length >= fn.length ? fn(...args) : (...more) => curried(...args, ...more); }; }".ai(),
        "export function partial(fn, ...preset) { return (...args) => fn(...preset, ...args); }".ai(),
        "export function flip(fn) { return (a, b, ...rest) => fn(b, a, ...rest); }".ai(),
        "export function tap(fn) { return x => { fn(x); return x; }; }".ai(),
        "export const constant = v => () => v;".ai(),
        "export const always = constant;".ai(),
        "export const negate = pred => (...args) => !pred(...args);".ai(),
    ]);
    let mut string_helper = repo.filename("helpers/string.js");
    string_helper.set_contents(crate::lines![
        "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);".ai(),
        "export const kebabToCamel = s => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase());".ai(),
        "export const camelToKebab = s => s.replace(/[A-Z]/g, m => `-${m.toLowerCase()}`);".ai(),
        "export const truncate = (s, n) => s.length <= n ? s : s.slice(0, n-3) + '...';".ai(),
        "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');".ai(),
        "export const words = s => s.trim().split(/\\s+/).filter(Boolean);".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 grow shared_util.js + helpers/string.js")
        .unwrap();

    // C3: append 8 more AI lines to shared_util.js + create helpers/array.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const identity = x => x;".ai(),
        "export const once = fn => { let c=false,r; return (...a) => { if(!c){c=true;r=fn(...a);} return r; }; };".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); return m.has(k)?m.get(k):(m.set(k,fn(...a)),m.get(k)); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "export const curry = fn => function c(...a) { return a.length>=fn.length ? fn(...a) : (...b)=>c(...a,...b); };".ai(),
        "export const partial = (fn,...p) => (...a) => fn(...p,...a);".ai(),
        "export const negate = pred => (...a) => !pred(...a);".ai(),
        "".ai(),
        "export function debounce(fn, ms) { let t; return (...a) => { clearTimeout(t); t = setTimeout(()=>fn(...a), ms); }; }".ai(),
        "export function throttle(fn, ms) { let ok=true; return (...a) => { if(ok) { ok=false; fn(...a); setTimeout(()=>ok=true,ms); } }; }".ai(),
        "export function trampoline(fn) { return (...a) => { let r=fn(...a); while(typeof r==='function') r=r(); return r; }; }".ai(),
        "export function juxt(...fns) { return (...a) => fns.map(f=>f(...a)); }".ai(),
        "export const when = (pred, fn) => (...a) => pred(...a) ? fn(...a) : a[0];".ai(),
    ]);
    let mut array_helper = repo.filename("helpers/array.js");
    array_helper.set_contents(crate::lines![
        "export const unique = arr => [...new Set(arr)];".ai(),
        "export const flatten = arr => arr.flat(Infinity);".ai(),
        "export const chunk = (arr, n) => Array.from({length: Math.ceil(arr.length/n)}, (_,i) => arr.slice(i*n, i*n+n));".ai(),
        "export const groupBy = (arr, key) => arr.reduce((g, item) => ((g[item[key]] ??= []).push(item), g), {});".ai(),
        "export const zip = (...arrays) => arrays[0].map((_,i) => arrays.map(a=>a[i]));".ai(),
        "export const intersection = (a, b) => a.filter(x => b.includes(x));".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 grow shared_util.js + helpers/array.js")
        .unwrap();

    // C4: append 8 more AI lines to shared_util.js + create helpers/object.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const identity = x => x;".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); return m.has(k)?m.get(k):(m.set(k,fn(...a)),m.get(k)); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "export const curry = fn => function c(...a) { return a.length>=fn.length ? fn(...a) : (...b)=>c(...a,...b); };".ai(),
        "export const debounce = (fn, ms) => { let t; return (...a)=>{ clearTimeout(t); t=setTimeout(()=>fn(...a),ms); }; };".ai(),
        "export const throttle = (fn, ms) => { let ok=true; return (...a)=>{ if(ok){ok=false;fn(...a);setTimeout(()=>ok=true,ms);} }; };".ai(),
        "export const when = (pred, fn) => (...a) => pred(...a) ? fn(...a) : a[0];".ai(),
        "".ai(),
        "export class EventEmitter { #events={}; on(e,f){(this.#events[e]??=[]).push(f);return this;} emit(e,...a){(this.#events[e]??[]).forEach(f=>f(...a));} }".ai(),
        "export const sleep = ms => new Promise(r => setTimeout(r, ms));".ai(),
        "export async function retry(fn, n=3) { for(let i=0;i<n;i++) { try{return await fn();}catch(e){if(i===n-1)throw e;await sleep(100*(i+1));} } }".ai(),
        "export const withTimeout = (p, ms) => Promise.race([p, new Promise((_,r)=>setTimeout(()=>r(new Error('timeout')),ms))]);".ai(),
        "export const deferred = () => { let res,rej; const p=new Promise((r,j)=>{res=r;rej=j;}); return {promise:p,resolve:res,reject:rej}; };".ai(),
    ]);
    let mut object_helper = repo.filename("helpers/object.js");
    object_helper.set_contents(crate::lines![
        "export const pick = (obj, keys) => Object.fromEntries(keys.map(k=>[k,obj[k]]));".ai(),
        "export const omit = (obj, keys) => Object.fromEntries(Object.entries(obj).filter(([k])=>!keys.includes(k)));".ai(),
        "export const deepClone = obj => JSON.parse(JSON.stringify(obj));".ai(),
        "export const isEmpty = obj => Object.keys(obj).length === 0;".ai(),
        "export const mapValues = (obj, fn) => Object.fromEntries(Object.entries(obj).map(([k,v])=>[k,fn(v,k)]));".ai(),
        "export const fromEntries = Object.fromEntries;".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 grow shared_util.js + helpers/object.js")
        .unwrap();

    // C5: append 8 more AI lines to shared_util.js + create helpers/number.js (6 AI lines)
    shared.set_contents(crate::lines![
        "export const VERSION = '1.0';",
        "".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const identity = x => x;".ai(),
        "export const memoize = fn => { const m=new Map(); return (...a)=>{ const k=JSON.stringify(a); return m.has(k)?m.get(k):(m.set(k,fn(...a)),m.get(k)); }; };".ai(),
        "export const pipe = (...fns) => x => fns.reduce((v,f)=>f(v),x);".ai(),
        "export const debounce = (fn, ms) => { let t; return (...a)=>{ clearTimeout(t); t=setTimeout(()=>fn(...a),ms); }; };".ai(),
        "export const sleep = ms => new Promise(r => setTimeout(r, ms));".ai(),
        "export async function retry(fn, n=3) { for(let i=0;i<n;i++) { try{return await fn();}catch(e){if(i===n-1)throw e;await sleep(100*(i+1));} } }".ai(),
        "export const deferred = () => { let res,rej; const p=new Promise((r,j)=>{res=r;rej=j;}); return {promise:p,resolve:res,reject:rej}; };".ai(),
        "".ai(),
        "export function deepEqual(a, b) {".ai(),
        "    if (a === b) return true;".ai(),
        "    if (typeof a !== typeof b) return false;".ai(),
        "    if (Array.isArray(a)) return a.length===b.length && a.every((v,i)=>deepEqual(v,b[i]));".ai(),
        "    if (typeof a === 'object' && a && b) {".ai(),
        "        const ka=Object.keys(a), kb=Object.keys(b);".ai(),
        "        return ka.length===kb.length && ka.every(k=>deepEqual(a[k],b[k]));".ai(),
        "    }".ai(),
        "    return false;".ai(),
        "}".ai(),
    ]);
    let mut number_helper = repo.filename("helpers/number.js");
    number_helper.set_contents(crate::lines![
        "export const round = (n, d=0) => Math.round(n * 10**d) / 10**d;".ai(),
        "export const clamp = (n, min, max) => Math.min(Math.max(n, min), max);".ai(),
        "export const lerp = (a, b, t) => a + (b - a) * t;".ai(),
        "export const isPrime = n => n>1 && Array.from({length:Math.sqrt(n)|0},(_, i)=>i+2).every(i=>n%i!==0);".ai(),
        "export const gcd = (a, b) => b === 0 ? a : gcd(b, a % b);".ai(),
        "export const formatBytes = n => { const u=['B','KB','MB','GB']; let i=0; while(n>=1024&&i<3){n/=1024;i++;} return `${n.toFixed(1)}${u[i]}`; };".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 grow shared_util.js + helpers/number.js")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {shared_util.js, helpers/date.js}; no future helpers
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(
        &repo,
        &chain[0],
        "sha0_files",
        &["shared_util.js", "helpers/date.js"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "helpers/string.js",
            "helpers/array.js",
            "helpers/object.js",
            "helpers/number.js",
        ],
    );

    // sha1 = C2': {shared_util.js, helpers/string.js}; no future helpers
    // C2 added curry/partial/flip/tap/negate to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["shared_util.js", "helpers/string.js"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["helpers/array.js", "helpers/object.js", "helpers/number.js"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "shared_util.js",
        "sha1_shared_curry",
        &[
            ("export function curry", true),
            ("export function partial", true),
            ("export const negate", true),
        ],
    );
    // helpers/date.js (from C1) is a prior file at chain[1] — fast path, verify attribution intact
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "helpers/date.js",
        "chain1_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );

    // sha2 = C3': {shared_util.js, helpers/array.js}; no object or number yet
    // C3 added debounce/throttle/trampoline/juxt/when to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["shared_util.js", "helpers/array.js"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["helpers/object.js", "helpers/number.js"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "shared_util.js",
        "sha2_shared_debounce",
        &[
            ("export function debounce", true),
            ("export function throttle", true),
            ("export function trampoline", true),
        ],
    );
    // helpers/date.js (from C1) and helpers/string.js (from C2) are prior files at chain[2]
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "helpers/date.js",
        "chain2_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "helpers/string.js",
        "chain2_prior_string_js",
        &[
            (
                "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);",
                true,
            ),
            (
                "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');",
                true,
            ),
        ],
    );

    // sha3 = C4': {shared_util.js, helpers/object.js}; no number yet
    // C4 added EventEmitter/sleep/retry/withTimeout/deferred to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(
        &repo,
        &chain[3],
        "sha3_files",
        &["shared_util.js", "helpers/object.js"],
    );
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["helpers/number.js"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "shared_util.js",
        "sha3_shared_eventemitter",
        &[
            ("export class EventEmitter", true),
            ("export const sleep", true),
            ("export async function retry", true),
        ],
    );
    // helpers/date.js (C1), helpers/string.js (C2), and helpers/array.js (C3) are prior files at chain[3]
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "helpers/date.js",
        "chain3_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "helpers/string.js",
        "chain3_prior_string_js",
        &[
            (
                "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);",
                true,
            ),
            (
                "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "helpers/array.js",
        "chain3_prior_array_js",
        &[
            ("export const unique = arr => [...new Set(arr)];", true),
            (
                "export const chunk = (arr, n) => Array.from({length: Math.ceil(arr.length/n)}, (_,i) => arr.slice(i*n, i*n+n));",
                true,
            ),
        ],
    );

    // sha4 = C5': {shared_util.js, helpers/number.js}
    // C5 added deepEqual to shared_util.js
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["shared_util.js", "helpers/number.js"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "shared_util.js",
        "sha4_shared_deepequal",
        &[
            ("export function deepEqual", true),
            ("if (Array.isArray(a))", true),
            ("return false;", true),
        ],
    );
    // helpers/date.js (C1), string.js (C2), array.js (C3), and object.js (C4) are prior files at chain[4]
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/date.js",
        "chain4_prior_date_js",
        &[
            ("export const now = () => new Date();", true),
            (
                "export const formatISO = d => d.toISOString().slice(0, 10);",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/string.js",
        "chain4_prior_string_js",
        &[
            (
                "export const capitalize = s => s.charAt(0).toUpperCase() + s.slice(1);",
                true,
            ),
            (
                "export const slugify = s => s.toLowerCase().trim().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/array.js",
        "chain4_prior_array_js",
        &[
            ("export const unique = arr => [...new Set(arr)];", true),
            (
                "export const chunk = (arr, n) => Array.from({length: Math.ceil(arr.length/n)}, (_,i) => arr.slice(i*n, i*n+n));",
                true,
            ),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "helpers/object.js",
        "chain4_prior_object_js",
        &[
            (
                "export const pick = (obj, keys) => Object.fromEntries(keys.map(k=>[k,obj[k]]));",
                true,
            ),
            (
                "export const deepClone = obj => JSON.parse(JSON.stringify(obj));",
                true,
            ),
        ],
    );
}

crate::reuse_tests_in_worktree!(
    test_slow_path_multiple_shared_files_both_modified,
    test_slow_path_mixed_unique_and_shared_files,
    test_slow_path_feature_has_human_commits_intermixed,
    test_slow_path_large_function_blocks_line_offset,
    test_slow_path_file_grows_then_unique_files_each_commit,
);
