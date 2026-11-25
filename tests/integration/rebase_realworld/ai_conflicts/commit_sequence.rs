use super::*;

/// Test 1: config.py TIMEOUT constant — feature (C3) changes TIMEOUT to 60,
/// main changes it to 120 → conflict.  AI resolves to TIMEOUT = 90.
/// C1' has users.py, C2' adds products.py, C3' adds config.py (AI-resolved),
/// C4' adds orders.py, C5' adds payments.py.
#[test]
fn test_conflict_ai_resolves_timeout_constant() {
    run_test_conflict_ai_resolves_timeout_constant(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_timeout_constant_standard_human() {
    run_test_conflict_ai_resolves_timeout_constant(HumanContextAttribution::Unattributed);
}

fn run_test_conflict_ai_resolves_timeout_constant(human_context: HumanContextAttribution) {
    let repo = TestRepo::new();

    // Initial: config.py with a class and TIMEOUT constant (human)
    repo.commit_untracked_file(
        "config.py",
        "class Config:\n    TIMEOUT = 30\n    HOST = 'localhost'\n    PORT = 8080\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: changes TIMEOUT to 120 and adds 4 more commits
    repo.commit_untracked_file(
        "config.py",
        "class Config:\n    TIMEOUT = 120\n    HOST = 'localhost'\n    PORT = 8080\n",
        "main: increase TIMEOUT to 120",
    );
    repo.commit_untracked_file(
        "logging_config.py",
        "import logging\nlogging.basicConfig(level=logging.INFO)\n",
        "main: add logging config",
    );
    repo.commit_untracked_file(
        "constants.py",
        "MAX_CONNECTIONS = 100\nDEFAULT_PAGE_SIZE = 20\n",
        "main: add constants",
    );
    repo.commit_untracked_file(
        "exceptions.py",
        "class AppError(Exception): pass\nclass ValidationError(AppError): pass\n",
        "main: add exceptions",
    );
    repo.commit_untracked_file(
        "utils.py",
        "def flatten(lst): return [x for sub in lst for x in sub]\n",
        "main: add utils",
    );

    // Feature branch from base (before main's TIMEOUT change)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates users.py (8 AI lines)
    let mut users = repo.filename("users.py");
    users.set_contents(crate::lines![
        "class UserService:".ai(),
        "    def __init__(self, db):".ai(),
        "        self.db = db".ai(),
        "    def get_user(self, uid):".ai(),
        "        return self.db.query('SELECT * FROM users WHERE id=?', uid)".ai(),
        "    def create_user(self, name, email):".ai(),
        "        return self.db.execute('INSERT INTO users VALUES (?, ?)', name, email)".ai(),
        "    def delete_user(self, uid):".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add user service")
        .unwrap();

    // C2: AI creates products.py (8 AI lines)
    let mut products = repo.filename("products.py");
    products.set_contents(crate::lines![
        "class ProductService:".ai(),
        "    def __init__(self, db):".ai(),
        "        self.db = db".ai(),
        "    def get_product(self, pid):".ai(),
        "        return self.db.query('SELECT * FROM products WHERE id=?', pid)".ai(),
        "    def list_products(self):".ai(),
        "        return self.db.query('SELECT * FROM products')".ai(),
        "    def update_price(self, pid, price):".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add product service")
        .unwrap();

    // C3: AI changes TIMEOUT to 60 in config.py — WILL CONFLICT with main's 120
    let mut config = repo.filename("config.py");
    config.set_contents(crate::lines![
        human_context.expected_line("class Config:"),
        "    TIMEOUT = 60".ai(),
        human_context.expected_line("    HOST = 'localhost'"),
        human_context.expected_line("    PORT = 8080"),
    ]);
    repo.stage_all_and_commit("feat: C3 AI tunes TIMEOUT to 60")
        .unwrap();

    // C4: AI creates orders.py (8 AI lines)
    let mut orders = repo.filename("orders.py");
    orders.set_contents(crate::lines![
        "class OrderService:".ai(),
        "    def __init__(self, db):".ai(),
        "        self.db = db".ai(),
        "    def create_order(self, uid, items):".ai(),
        "        total = sum(i['price'] for i in items)".ai(),
        "        return self.db.execute('INSERT INTO orders VALUES (?, ?)', uid, total)".ai(),
        "    def get_order(self, oid):".ai(),
        "        return self.db.query('SELECT * FROM orders WHERE id=?', oid)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add order service")
        .unwrap();

    // C5: AI creates payments.py (8 AI lines)
    let mut payments = repo.filename("payments.py");
    payments.set_contents(crate::lines![
        "class PaymentService:".ai(),
        "    def __init__(self, db, stripe):".ai(),
        "        self.db = db".ai(),
        "        self.stripe = stripe".ai(),
        "    def charge(self, oid, amount, token):".ai(),
        "        r = self.stripe.charge(amount, token)".ai(),
        "        self.db.execute('INSERT INTO payments VALUES (?, ?)', oid, r['id'])".ai(),
        "    def refund(self, pid):".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add payment service")
        .unwrap();

    // Rebase onto main — C3 will conflict on config.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on config.py at C3"
    );

    // AI resolves: sets TIMEOUT = 90 as .ai(), surrounding lines as the selected human attribution
    let mut conflict_config = repo.filename("config.py");
    conflict_config.set_contents(crate::lines![
        human_context.expected_line("class Config:"),
        "    TIMEOUT = 90".ai(),
        human_context.expected_line("    HOST = 'localhost'"),
        human_context.expected_line("    PORT = 8080"),
    ]);
    // set_contents already ran git add -A + checkpoint
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': users.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["users.py"]);

    // C2': products.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["products.py"]);

    // C3': config.py only (AI-resolved, TIMEOUT = 90 attributed as AI)
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["config.py"]);
    // 1 AI line: TIMEOUT = 90 (working-log fallback path must set accepted_lines correctly)
    assert_accepted_lines_exact(&repo, &chain[2], "c3_accepted_lines", 1);

    // blame at chain[2] for config.py: the AI-resolved TIMEOUT line should be AI
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "config.py",
        "c3_blame_config",
        &[
            ("class Config:", false),
            ("TIMEOUT = 90", true),
            ("HOST = 'localhost'", false),
            ("PORT = 8080", false),
        ],
    );

    // C4': orders.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["orders.py"]);

    // C5': payments.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["payments.py"]);

    human_context.assert_metadata_humans(&repo, &chain[2], "c3'");
}

/// Test 2: compute.rs function body — feature (C2) implements a function with
/// 10 AI lines, main also implements it differently (conflict).  AI resolution
/// produces 15 merged lines (all .ai()).  Extra lines from resolution are counted.
#[test]
fn test_conflict_ai_resolves_with_added_extra_lines() {
    let repo = TestRepo::new();

    // Initial: compute.rs with a function stub (human)
    repo.commit_untracked_file(
        "src/compute.rs",
        "pub fn compute(data: &[f64]) -> f64 { 0.0 }\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: implements the function differently (human) → will conflict
    repo.commit_untracked_file("src/compute.rs",
        "pub fn compute(data: &[f64]) -> f64 {\n    data.iter().sum::<f64>() / data.len() as f64\n}\n",
        "main: implement compute as mean",
    );
    repo.commit_untracked_file(
        "src/main.rs",
        "fn main() { println!(\"hello\"); }\n",
        "main: add main",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"compute\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "src/tests.rs",
        "#[cfg(test)]\nmod tests { #[test] fn it_works() {} }\n",
        "main: add tests",
    );
    repo.commit_untracked_file(
        "README.md",
        "# compute\nA compute library.\n",
        "main: add README",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates types.rs (8 AI lines)
    let mut types = repo.filename("src/types.rs");
    types.set_contents(crate::lines![
        "#[derive(Debug, Clone, PartialEq)]".ai(),
        "pub struct DataPoint {".ai(),
        "    pub value: f64,".ai(),
        "    pub weight: f64,".ai(),
        "}".ai(),
        "".ai(),
        "impl DataPoint {".ai(),
        "    pub fn new(value: f64, weight: f64) -> Self { Self { value, weight } }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add DataPoint type")
        .unwrap();

    // C2: AI implements compute.rs with 10 AI lines — WILL CONFLICT with main's implementation
    let mut compute = repo.filename("src/compute.rs");
    compute.set_contents(crate::lines![
        "pub fn compute(data: &[f64]) -> f64 {".ai(),
        "    if data.is_empty() { return 0.0; }".ai(),
        "    let n = data.len() as f64;".ai(),
        "    let mean = data.iter().sum::<f64>() / n;".ai(),
        "    let variance = data.iter()".ai(),
        "        .map(|x| (x - mean).powi(2))".ai(),
        "        .sum::<f64>() / n;".ai(),
        "    variance.sqrt()".ai(),
        "}".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 AI implements compute as std-dev")
        .unwrap();

    // C3: AI creates validator.rs (8 AI lines)
    let mut validator = repo.filename("src/validator.rs");
    validator.set_contents(crate::lines![
        "pub fn validate_data(data: &[f64]) -> Result<(), String> {".ai(),
        "    if data.is_empty() { return Err(\"empty data\".into()); }".ai(),
        "    if data.iter().any(|x| x.is_nan()) { return Err(\"NaN in data\".into()); }".ai(),
        "    if data.iter().any(|x| x.is_infinite()) { return Err(\"Inf in data\".into()); }".ai(),
        "    Ok(())".ai(),
        "}".ai(),
        "".ai(),
        "pub fn normalize(data: &mut Vec<f64>) { let m = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max); data.iter_mut().for_each(|x| *x /= m); }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add data validator")
        .unwrap();

    // C4: AI creates encoder.rs (8 AI lines)
    let mut encoder = repo.filename("src/encoder.rs");
    encoder.set_contents(crate::lines![
        "pub fn encode(data: &[f64]) -> Vec<u8> {".ai(),
        "    data.iter()".ai(),
        "        .flat_map(|x| x.to_le_bytes())".ai(),
        "        .collect()".ai(),
        "}".ai(),
        "".ai(),
        "pub fn decode(bytes: &[u8]) -> Vec<f64> {".ai(),
        "    bytes.chunks_exact(8).map(|c| f64::from_le_bytes(c.try_into().unwrap())).collect()"
            .ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add encoder").unwrap();

    // C5: AI creates decoder.rs (8 AI lines)
    let mut decoder = repo.filename("src/decoder.rs");
    decoder.set_contents(crate::lines![
        "use crate::encoder::decode;".ai(),
        "".ai(),
        "pub struct Decoder {".ai(),
        "    buffer: Vec<u8>,".ai(),
        "}".ai(),
        "".ai(),
        "impl Decoder {".ai(),
        "    pub fn new() -> Self { Self { buffer: Vec::new() } }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add decoder struct")
        .unwrap();

    // Rebase onto main — C2 will conflict on src/compute.rs
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/compute.rs at C2"
    );

    // AI resolves: writes a 15-line merged implementation (all .ai())
    let mut conflict_compute = repo.filename("src/compute.rs");
    conflict_compute.set_contents(crate::lines![
        "pub fn compute(data: &[f64]) -> f64 {".ai(),
        "    if data.is_empty() { return 0.0; }".ai(),
        "    let n = data.len() as f64;".ai(),
        "    let mean = data.iter().sum::<f64>() / n;".ai(),
        "    let variance = data.iter()".ai(),
        "        .map(|x| (x - mean).powi(2))".ai(),
        "        .sum::<f64>() / n;".ai(),
        "    let std_dev = variance.sqrt();".ai(),
        "    // Also return weighted mean as a combined metric".ai(),
        "    let weighted_sum: f64 = data.iter().enumerate().map(|(i, x)| x * (i + 1) as f64).sum();".ai(),
        "    let weight_total: f64 = (1..=data.len()).map(|i| i as f64).sum();".ai(),
        "    let weighted_mean = weighted_sum / weight_total;".ai(),
        "    std_dev * 0.5 + weighted_mean * 0.5".ai(),
        "}".ai(),
        "".ai(),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': types.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/types.rs"]);

    // C2': compute.rs only (AI-resolved)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/compute.rs"]);

    // blame at chain[1] for compute.rs:
    // Line 1 and "}" are unchanged from C2's parent (main branch version),
    // so git-blame traces them to the main branch commit (no note → human).
    // All other lines are new in C2' and the AI checkpoint captured them → AI.
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "src/compute.rs",
        "c2_blame_compute",
        &[
            ("pub fn compute", false), // unchanged from parent, traces to main branch commit (human)
            ("is_empty", true),        // new in C2', AI per checkpoint
            ("let n =", true),
            ("let mean =", true),
            ("variance = data", true),
            (".map(|x|", true),
            (".sum::<f64>", true),
            ("let std_dev", true), // new in C2', AI per checkpoint
            ("weighted mean", true),
            ("weighted_sum", true),
            ("weight_total:", true),
            ("weighted_mean =", true),
            ("std_dev * 0.5", true),
            ("}", false), // unchanged from parent ("}" line), traces to main branch (human)
        ],
    );

    // C3': validator.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/validator.rs"]);

    // C4': encoder.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/encoder.rs"]);

    // C5': decoder.rs only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/decoder.rs"]);
}

/// Test 3: processor.py — feature (C3) adds 5 AI lines to method2 body,
/// main also changes method2.  AI resolution rewrites processor.py preserving
/// 2 human context lines and writing 7 lines for the resolved method2 (marked
/// `.ai()` in set_contents).  However, the content-diff path only carries
/// attribution for lines whose content exactly matches the original feature commit:
/// only `def method2(self):`, `result = []`, `for i in range(10):`, and
/// `result.append(i * 2)` survive the content match — 4 lines.  The newly
/// introduced lines (`# AI merged`, `label = `, `return result, label`) have no
/// entry in `original_head_line_to_author` and therefore receive human attribution.
#[test]
fn test_conflict_ai_resolves_preserving_human_context_lines() {
    run_test_conflict_ai_resolves_preserving_human_context_lines(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_preserving_human_context_lines_standard_human() {
    run_test_conflict_ai_resolves_preserving_human_context_lines(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_preserving_human_context_lines(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: processor.py with a class (6 human lines)
    repo.commit_untracked_file("processor.py",
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self): pass\n    def method3(self): return 'method3'\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: human changes method2 differently → conflict
    repo.commit_untracked_file("processor.py",
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self): return 'human-method2'\n    def method3(self): return 'method3'\n",
        "main: implement method2",
    );
    repo.commit_untracked_file(
        "runner.py",
        "from processor import Processor\np = Processor()\np.method1()\n",
        "main: add runner",
    );
    repo.commit_untracked_file("tests/test_processor.py",
        "from processor import Processor\ndef test_method1(): assert Processor().method1() == 'method1'\n",
        "main: add tests",
    );
    repo.commit_untracked_file(
        "setup.py",
        "from setuptools import setup\nsetup(name='processor', version='0.1.0')\n",
        "main: add setup.py",
    );
    repo.commit_untracked_file(
        "pyproject.toml",
        "[build-system]\nrequires = ['setuptools']\n",
        "main: add pyproject.toml",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates util_a.py (8 AI lines)
    let mut util_a = repo.filename("util_a.py");
    util_a.set_contents(crate::lines![
        "def parse_int(s: str) -> int:".ai(),
        "    try:".ai(),
        "        return int(s)".ai(),
        "    except ValueError:".ai(),
        "        raise ValueError(f'Cannot parse {s!r} as int')".ai(),
        "".ai(),
        "def parse_float(s: str) -> float:".ai(),
        "    return float(s)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add util_a").unwrap();

    // C2: AI creates util_b.py (8 AI lines)
    let mut util_b = repo.filename("util_b.py");
    util_b.set_contents(crate::lines![
        "from typing import List, Optional".ai(),
        "".ai(),
        "def chunk(lst: List, size: int) -> List[List]:".ai(),
        "    return [lst[i:i+size] for i in range(0, len(lst), size)]".ai(),
        "".ai(),
        "def flatten(lst: List[List]) -> List:".ai(),
        "    return [x for sub in lst for x in sub]".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add util_b").unwrap();

    // C3: AI adds 5 lines to method2 in processor.py — WILL CONFLICT
    let mut processor = repo.filename("processor.py");
    processor.set_contents(crate::lines![
        human_context.expected_line("class Processor:"),
        human_context.expected_line("    def method1(self): return 'method1'"),
        "    def method2(self):".ai(),
        "        result = []".ai(),
        "        for i in range(10):".ai(),
        "            result.append(i * 2)".ai(),
        "        return result".ai(),
        human_context.expected_line("    def method3(self): return 'method3'"),
    ]);
    fs::write(
        repo.path().join("processor.py"),
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self):\n        result = []\n        for i in range(10):\n            result.append(i * 2)\n        return result\n    def method3(self): return 'method3'\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "processor.py"])
        .unwrap();
    repo.stage_all_and_commit("feat: C3 AI implements method2")
        .unwrap();

    // C4: AI creates util_d.py (8 AI lines)
    let mut util_d = repo.filename("util_d.py");
    util_d.set_contents(crate::lines![
        "import hashlib".ai(),
        "".ai(),
        "def md5(s: str) -> str:".ai(),
        "    return hashlib.md5(s.encode()).hexdigest()".ai(),
        "".ai(),
        "def sha256(s: str) -> str:".ai(),
        "    return hashlib.sha256(s.encode()).hexdigest()".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add util_d").unwrap();

    // C5: AI creates util_e.py (8 AI lines)
    let mut util_e = repo.filename("util_e.py");
    util_e.set_contents(crate::lines![
        "import json".ai(),
        "".ai(),
        "def to_json(obj) -> str:".ai(),
        "    return json.dumps(obj, indent=2)".ai(),
        "".ai(),
        "def from_json(s: str):".ai(),
        "    return json.loads(s)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add util_e").unwrap();

    // Rebase — C3 will conflict on processor.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on processor.py at C3"
    );

    // AI resolves: 2 human context lines + 7 lines for resolved method2 (set_contents(.ai()))
    // NOTE: content-diff only recovers lines matching original C3 content:
    //   def method2, result = [], for i in range, result.append → 4 AI-attributed lines.
    //   # AI merged, label = , return result/label → newly introduced, no original match → human.
    let mut conflict_processor = repo.filename("processor.py");
    conflict_processor.set_contents(crate::lines![
        human_context.expected_line("class Processor:"),
        human_context.expected_line("    def method1(self): return 'method1'"),
        "    def method2(self):".ai(),
        "        # AI merged: combines human's return with feature's loop".ai(),
        "        result = []".ai(),
        "        for i in range(10):".ai(),
        "            result.append(i * 2)".ai(),
        "        label = 'human-method2'".ai(),
        "        return result, label".ai(),
        human_context.expected_line("    def method3(self): return 'method3'"),
    ]);
    fs::write(
        repo.path().join("processor.py"),
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self):\n        # AI merged: combines human's return with feature's loop\n        result = []\n        for i in range(10):\n            result.append(i * 2)\n        label = 'human-method2'\n        return result, label\n    def method3(self): return 'method3'\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "processor.py"])
        .unwrap();
    repo.git(&["add", "processor.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': util_a.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["util_a.py"]);

    // C2': util_b.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["util_b.py"]);

    // C3': processor.py only (AI-resolved: 4 AI lines via content-diff match)
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["processor.py"]);

    // blame at chain[2] for processor.py: lines from parent are human,
    // all new lines written by AI during resolution are AI.
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "processor.py",
        "c3_blame_processor",
        &[
            ("class Processor:", false),
            ("def method1", false),
            ("def method2", true),
            ("AI merged", true),
            ("result = []", true),
            ("for i in range", true),
            ("result.append", true),
            ("label = ", true),
            ("return result, label", true),
            ("def method3", false),
        ],
    );

    // C4': util_d.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["util_d.py"]);

    // C5': util_e.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["util_e.py"]);

    human_context.assert_metadata_humans(&repo, &chain[2], "c3'");
}

/// Test 4: version.py — conflict is on C1 (the VERY FIRST feature commit).
/// Feature changes VERSION to "2.0", main changes it to "1.5".
/// AI resolves to "2.1".  C2–C5 accumulate other files normally.
#[test]
fn test_conflict_ai_resolves_on_first_commit() {
    run_test_conflict_ai_resolves_on_first_commit(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_on_first_commit_standard_human() {
    run_test_conflict_ai_resolves_on_first_commit(HumanContextAttribution::Unattributed);
}

fn run_test_conflict_ai_resolves_on_first_commit(human_context: HumanContextAttribution) {
    let repo = TestRepo::new();

    // Initial: version.py with VERSION = "1.0"
    repo.commit_untracked_file(
        "version.py",
        "VERSION = \"1.0\"\nCODENAME = \"alpha\"\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: changes VERSION to "1.5" — will conflict with feature's C1
    repo.commit_untracked_file(
        "version.py",
        "VERSION = \"1.5\"\nCODENAME = \"beta\"\n",
        "main: bump version to 1.5",
    );
    repo.commit_untracked_file(
        "CHANGELOG.md",
        "## 1.5\n- Performance improvements\n",
        "main: add changelog",
    );
    repo.commit_untracked_file(
        "CONTRIBUTORS.md",
        "# Contributors\n- Alice\n- Bob\n",
        "main: add contributors",
    );
    repo.commit_untracked_file(
        "LICENSE",
        "MIT License\nCopyright 2024\n",
        "main: add license",
    );
    repo.commit_untracked_file(
        "docs/index.md",
        "# Docs\nWelcome to the docs.\n",
        "main: add docs",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI changes VERSION to "2.0" — WILL CONFLICT
    let mut version = repo.filename("version.py");
    version.set_contents(crate::lines![
        "VERSION = \"2.0\"".ai(),
        human_context.expected_line("CODENAME = \"alpha\""),
    ]);
    repo.stage_all_and_commit("feat: C1 bump version to 2.0")
        .unwrap();

    // C2: AI creates changelog.py (8 AI lines)
    let mut changelog = repo.filename("changelog.py");
    changelog.set_contents(crate::lines![
        "import datetime".ai(),
        "".ai(),
        "class ChangelogEntry:".ai(),
        "    def __init__(self, version: str, date: datetime.date, changes: list):".ai(),
        "        self.version = version".ai(),
        "        self.date = date".ai(),
        "        self.changes = changes".ai(),
        "    def render(self) -> str: return f'{self.version} ({self.date}): {len(self.changes)} changes'".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add changelog model")
        .unwrap();

    // C3: AI creates release_notes.py (8 AI lines)
    let mut release_notes = repo.filename("release_notes.py");
    release_notes.set_contents(crate::lines![
        "from typing import List".ai(),
        "".ai(),
        "def format_release_notes(entries: List[dict]) -> str:".ai(),
        "    lines = []".ai(),
        "    for e in entries:".ai(),
        "        lines.append(f\"## {e['version']}\")".ai(),
        "        for change in e.get('changes', []):".ai(),
        "            lines.append(f'- {change}')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add release notes formatter")
        .unwrap();

    // C4: AI creates deprecations.py (8 AI lines)
    let mut deprecations = repo.filename("deprecations.py");
    deprecations.set_contents(crate::lines![
        "import warnings".ai(),
        "import functools".ai(),
        "".ai(),
        "def deprecated(reason: str):".ai(),
        "    def decorator(func):".ai(),
        "        @functools.wraps(func)".ai(),
        "        def wrapper(*args, **kwargs):".ai(),
        "            warnings.warn(f'{func.__name__} is deprecated: {reason}', DeprecationWarning, stacklevel=2)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add deprecation decorator")
        .unwrap();

    // C5: AI creates migration_guide.py (8 AI lines)
    let mut migration_guide = repo.filename("migration_guide.py");
    migration_guide.set_contents(crate::lines![
        "MIGRATION_STEPS = [".ai(),
        "    'Update config files to new schema',".ai(),
        "    'Run database migration scripts',".ai(),
        "    'Update API call signatures',".ai(),
        "    'Test all integrations',".ai(),
        "    'Deploy to staging first',".ai(),
        "    'Monitor error rates after deployment',".ai(),
        "]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add migration guide")
        .unwrap();

    // Rebase — C1 will conflict immediately on version.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on version.py at C1"
    );

    // AI resolves: VERSION = "2.1" as .ai(), CODENAME as the selected human attribution
    let mut conflict_version = repo.filename("version.py");
    conflict_version.set_contents(crate::lines![
        "VERSION = \"2.1\"".ai(),
        human_context.expected_line("CODENAME = \"beta\""),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': version.py only with AI-resolved VERSION line (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["version.py"]);

    // blame at chain[0] for version.py: VERSION line is AI, CODENAME is human
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "version.py",
        "c1_blame_version",
        &[("VERSION = \"2.1\"", true), ("CODENAME = \"beta\"", false)],
    );

    // C2': changelog.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["changelog.py"]);

    // C3': release_notes.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["release_notes.py"]);

    // C4': deprecations.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["deprecations.py"]);

    // C5': migration_guide.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["migration_guide.py"]);

    human_context.assert_metadata_humans(&repo, &chain[0], "c1'");
}

/// Test 5: schema.rs max_connections — conflict is on C5 (LAST feature commit).
/// C1–C4 accumulate model_*.rs files cleanly.  C5 modifies schema.rs
/// max_connections constant; main also modifies same constant.  AI resolves.
#[test]
fn test_conflict_ai_resolves_on_last_commit() {
    run_test_conflict_ai_resolves_on_last_commit(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_on_last_commit_standard_human() {
    run_test_conflict_ai_resolves_on_last_commit(HumanContextAttribution::Unattributed);
}

fn run_test_conflict_ai_resolves_on_last_commit(human_context: HumanContextAttribution) {
    let repo = TestRepo::new();

    // Initial: schema.rs with a constant (human)
    repo.commit_untracked_file(
        "src/schema.rs",
        "pub const MAX_CONNECTIONS: u32 = 10;\npub const SCHEMA_VERSION: u32 = 1;\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: changes max_connections → will conflict with feature's C5
    repo.commit_untracked_file(
        "src/schema.rs",
        "pub const MAX_CONNECTIONS: u32 = 50;\npub const SCHEMA_VERSION: u32 = 1;\n",
        "main: increase max_connections to 50",
    );
    repo.commit_untracked_file(
        "src/migration.rs",
        "pub fn run_migrations() {}\n",
        "main: add migration runner",
    );
    repo.commit_untracked_file(
        "src/connection.rs",
        "pub struct Connection { id: u32 }\n",
        "main: add Connection type",
    );
    repo.commit_untracked_file(
        "src/pool.rs",
        "pub struct Pool { size: u32 }\n",
        "main: add Pool struct",
    );
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"schema\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "main: add Cargo.toml",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates model_a.rs (10 AI lines)
    let mut model_a = repo.filename("src/model_a.rs");
    model_a.set_contents(crate::lines![
        "#[derive(Debug, Clone)]".ai(),
        "pub struct ModelA {".ai(),
        "    pub id: u64,".ai(),
        "    pub name: String,".ai(),
        "    pub active: bool,".ai(),
        "}".ai(),
        "".ai(),
        "impl ModelA {".ai(),
        "    pub fn new(id: u64, name: impl Into<String>) -> Self {".ai(),
        "        Self { id, name: name.into(), active: true }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add ModelA").unwrap();

    // C2: AI creates model_b.rs (10 AI lines)
    let mut model_b = repo.filename("src/model_b.rs");
    model_b.set_contents(crate::lines![
        "#[derive(Debug, Clone)]".ai(),
        "pub struct ModelB {".ai(),
        "    pub id: u64,".ai(),
        "    pub value: f64,".ai(),
        "    pub tags: Vec<String>,".ai(),
        "}".ai(),
        "".ai(),
        "impl ModelB {".ai(),
        "    pub fn new(id: u64, value: f64) -> Self {".ai(),
        "        Self { id, value, tags: Vec::new() }".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add ModelB").unwrap();

    // C3: AI creates model_c.rs (10 AI lines)
    let mut model_c = repo.filename("src/model_c.rs");
    model_c.set_contents(crate::lines![
        "#[derive(Debug, Clone, PartialEq)]".ai(),
        "pub enum Status {".ai(),
        "    Active,".ai(),
        "    Inactive,".ai(),
        "    Pending,".ai(),
        "}".ai(),
        "".ai(),
        "impl Default for Status {".ai(),
        "    fn default() -> Self { Status::Pending }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add Status enum")
        .unwrap();

    // C4: AI creates model_d.rs (10 AI lines)
    let mut model_d = repo.filename("src/model_d.rs");
    model_d.set_contents(crate::lines![
        "use std::collections::HashMap;".ai(),
        "".ai(),
        "#[derive(Debug, Default)]".ai(),
        "pub struct Registry {".ai(),
        "    entries: HashMap<u64, String>,".ai(),
        "}".ai(),
        "".ai(),
        "impl Registry {".ai(),
        "    pub fn register(&mut self, id: u64, name: impl Into<String>) {".ai(),
        "        self.entries.insert(id, name.into());".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Registry").unwrap();

    // C5: AI changes max_connections to 100 — WILL CONFLICT
    let mut schema = repo.filename("src/schema.rs");
    schema.set_contents(crate::lines![
        "pub const MAX_CONNECTIONS: u32 = 100;".ai(),
        human_context.expected_line("pub const SCHEMA_VERSION: u32 = 1;"),
    ]);
    fs::write(
        repo.path().join("src/schema.rs"),
        "pub const MAX_CONNECTIONS: u32 = 100;\npub const SCHEMA_VERSION: u32 = 1;\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/schema.rs"])
        .unwrap();
    repo.stage_all_and_commit("feat: C5 AI tunes MAX_CONNECTIONS to 100")
        .unwrap();

    // Rebase — C5 will conflict on src/schema.rs
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on src/schema.rs at C5"
    );

    // AI resolves: picks 75 as a compromise, as .ai()
    let mut conflict_schema = repo.filename("src/schema.rs");
    conflict_schema.set_contents(crate::lines![
        "pub const MAX_CONNECTIONS: u32 = 75;".ai(),
        human_context.expected_line("pub const SCHEMA_VERSION: u32 = 1;"),
    ]);
    fs::write(
        repo.path().join("src/schema.rs"),
        "pub const MAX_CONNECTIONS: u32 = 75;\npub const SCHEMA_VERSION: u32 = 1;\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "src/schema.rs"])
        .unwrap();
    repo.git(&["add", "src/schema.rs"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': model_a.rs only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["src/model_a.rs"]);

    // C2': model_b.rs only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["src/model_b.rs"]);

    // C3': model_c.rs only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["src/model_c.rs"]);

    // C4': model_d.rs only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["src/model_d.rs"]);

    // C5': schema.rs only (AI-resolved MAX_CONNECTIONS)
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["src/schema.rs"]);

    // blame at chain[4] for schema.rs: MAX_CONNECTIONS line is AI, SCHEMA_VERSION is human
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "src/schema.rs",
        "c5_blame_schema",
        &[
            ("MAX_CONNECTIONS: u32 = 75", true),
            ("SCHEMA_VERSION: u32 = 1", false),
        ],
    );

    human_context.assert_metadata_humans(&repo, &chain[4], "c5'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_timeout_constant,
    test_conflict_ai_resolves_timeout_constant_standard_human,
    test_conflict_ai_resolves_with_added_extra_lines,
    test_conflict_ai_resolves_preserving_human_context_lines,
    test_conflict_ai_resolves_preserving_human_context_lines_standard_human,
    test_conflict_ai_resolves_on_first_commit,
    test_conflict_ai_resolves_on_first_commit_standard_human,
    test_conflict_ai_resolves_on_last_commit,
    test_conflict_ai_resolves_on_last_commit_standard_human,
);
