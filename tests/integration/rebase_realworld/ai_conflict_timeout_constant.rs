use super::{
    ExpectedLineExt, HumanContextAttribution, TestRepo, assert_accepted_lines_exact,
    assert_blame_at_commit, assert_note_base_commit_matches, assert_note_files_exact,
    get_commit_chain,
};

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

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_timeout_constant,
    test_conflict_ai_resolves_timeout_constant_standard_human,
);
