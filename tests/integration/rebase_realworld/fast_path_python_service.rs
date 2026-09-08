use super::{
    ExpectedLineExt, TestRepo, assert_blame_at_commit, assert_blame_sample_at_commit,
    assert_note_base_commit_matches, assert_note_files_exact, assert_note_no_forbidden_files,
    get_commit_chain,
};

// ============================================================================
// Category 1: Fast Path rebase tests
// Feature and main branches touch COMPLETELY DIFFERENT files so blob OIDs
// are identical between original and rebased commits (fast path fires).
// ============================================================================

#[test]
fn test_fast_path_python_microservice_5_endpoints() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("init.py");
    init.set_contents(crate::lines!["# microservice project init"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a new AI-generated service file ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: users.py
    let mut f1 = repo.filename("users.py");
    f1.set_contents(crate::lines![
        "class UserService:".ai(),
        "    def __init__(self, db):".ai(),
        "        self.db = db".ai(),
        "        self.cache = {}".ai(),
        "    def get_user(self, user_id):".ai(),
        "        if user_id in self.cache:".ai(),
        "            return self.cache[user_id]".ai(),
        "        return self.db.query('SELECT * FROM users WHERE id = ?', user_id)".ai(),
        "    def create_user(self, name, email):".ai(),
        "        return self.db.execute('INSERT INTO users (name, email) VALUES (?, ?)', name, email)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add user service").unwrap();

    // C2: products.py
    let mut f2 = repo.filename("products.py");
    f2.set_contents(crate::lines![
        "class ProductService:".ai(),
        "    def __init__(self, db):".ai(),
        "        self.db = db".ai(),
        "        self.index = {}".ai(),
        "    def get_product(self, product_id):".ai(),
        "        return self.db.query('SELECT * FROM products WHERE id = ?', product_id)".ai(),
        "    def list_products(self, category=None):".ai(),
        "        if category:".ai(),
        "            return self.db.query('SELECT * FROM products WHERE category = ?', category)"
            .ai(),
        "        return self.db.query('SELECT * FROM products')".ai(),
    ]);
    repo.stage_all_and_commit("feat: add product service")
        .unwrap();

    // C3: orders.py
    let mut f3 = repo.filename("orders.py");
    f3.set_contents(crate::lines![
        "class OrderService:".ai(),
        "    def __init__(self, db, user_svc, product_svc):".ai(),
        "        self.db = db".ai(),
        "        self.user_svc = user_svc".ai(),
        "        self.product_svc = product_svc".ai(),
        "    def create_order(self, user_id, items):".ai(),
        "        user = self.user_svc.get_user(user_id)".ai(),
        "        total = sum(self.product_svc.get_product(i['id'])['price'] * i['qty'] for i in items)".ai(),
        "        return self.db.execute('INSERT INTO orders (user_id, total) VALUES (?, ?)', user_id, total)".ai(),
        "    def get_order(self, order_id):".ai(),
    ]);
    repo.stage_all_and_commit("feat: add order service")
        .unwrap();

    // C4: payments.py
    let mut f4 = repo.filename("payments.py");
    f4.set_contents(crate::lines![
        "class PaymentService:".ai(),
        "    def __init__(self, db, stripe_client):".ai(),
        "        self.db = db".ai(),
        "        self.stripe = stripe_client".ai(),
        "    def charge(self, order_id, amount_cents, card_token):".ai(),
        "        result = self.stripe.charge.create(amount=amount_cents, currency='usd', source=card_token)".ai(),
        "        self.db.execute('INSERT INTO payments (order_id, stripe_id) VALUES (?, ?)', order_id, result['id'])".ai(),
        "        return result".ai(),
        "    def refund(self, payment_id):".ai(),
        "        return self.stripe.refund.create(charge=payment_id)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add payment service")
        .unwrap();

    // C5: webhooks.py
    let mut f5 = repo.filename("webhooks.py");
    f5.set_contents(crate::lines![
        "class WebhookService:".ai(),
        "    def __init__(self, db, http_client):".ai(),
        "        self.db = db".ai(),
        "        self.http = http_client".ai(),
        "    def register(self, url, events):".ai(),
        "        return self.db.execute('INSERT INTO webhooks (url, events) VALUES (?, ?)', url, ','.join(events))".ai(),
        "    def dispatch(self, event, payload):".ai(),
        "        hooks = self.db.query('SELECT * FROM webhooks WHERE events LIKE ?', f'%{event}%')".ai(),
        "        for hook in hooks:".ai(),
        "            self.http.post(hook['url'], json=payload)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add webhook service")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on DIFFERENT files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "tests/test_base.py",
        "import unittest\nclass BaseTest(unittest.TestCase): pass\n",
        "test: add base test class",
    );
    repo.commit_untracked_file(".github/ci.yml",
        "name: CI\non: [push, pull_request]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - uses: actions/checkout@v3\n      - run: python -m pytest\n",
        "ci: add github actions workflow",
    );
    repo.commit_untracked_file(
        "conftest.py",
        "import pytest\n\n@pytest.fixture\ndef db():\n    return MockDatabase()\n",
        "test: add pytest conftest",
    );
    repo.commit_untracked_file(
        "Makefile",
        "test:\n\tpython -m pytest tests/\nlint:\n\tflake8 .\n.PHONY: test lint\n",
        "build: add Makefile",
    );
    repo.commit_untracked_file(
        "setup.cfg",
        "[metadata]\nname = microservice\nversion = 0.1.0\n[options]\npython_requires = >=3.9\n",
        "build: add setup.cfg",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT in the rebased chain ===
    let chain = get_commit_chain(&repo, 5);
    // chain[0]=HEAD~4 (C1'), chain[1]=HEAD~3 (C2'), ..., chain[4]=HEAD (C5')

    // sha0 = C1': only users.py
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["users.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &["products.py", "orders.py", "payments.py", "webhooks.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "users.py",
        "sha0_blame",
        &[
            ("class UserService:", true),
            ("def __init__(self, db):", true),
            ("self.db = db", true),
            ("self.cache = {}", true),
            ("def get_user(self, user_id):", true),
            ("if user_id in self.cache:", true),
            ("return self.cache[user_id]", true),
            ("SELECT * FROM users WHERE id = ?", true),
            ("def create_user(self, name, email):", true),
            ("INSERT INTO users", true),
        ],
    );

    // sha1 = C2': products.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["products.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["orders.py", "payments.py", "webhooks.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "products.py",
        "sha1_blame",
        &[
            ("class ProductService:", true),
            ("def __init__(self, db):", true),
            ("self.db = db", true),
            ("self.index = {}", true),
            ("def get_product(self, product_id):", true),
            ("SELECT * FROM products WHERE id = ?", true),
            ("def list_products(self, category=None):", true),
            ("if category:", true),
            ("SELECT * FROM products WHERE category = ?", true),
            ("SELECT * FROM products", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "users.py",
        "chain1_prior_users_py",
        &[
            ("class UserService:", true),
            ("def get_user(self, user_id):", true),
        ],
    );

    // sha2 = C3': orders.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["orders.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["payments.py", "webhooks.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "orders.py",
        "sha2_blame",
        &[
            ("class OrderService:", true),
            ("def __init__(self, db, user_svc, product_svc):", true),
            ("self.db = db", true),
            ("self.user_svc = user_svc", true),
            ("self.product_svc = product_svc", true),
            ("def create_order(self, user_id, items):", true),
            ("user = self.user_svc.get_user(user_id)", true),
            ("total = sum", true),
            ("INSERT INTO orders", true),
            ("def get_order(self, order_id):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "users.py",
        "chain2_prior_users_py",
        &[
            ("class UserService:", true),
            ("def get_user(self, user_id):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "products.py",
        "chain2_prior_products_py",
        &[
            ("class ProductService:", true),
            ("def list_products(self, category=None):", true),
        ],
    );

    // sha3 = C4': payments.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["payments.py"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["webhooks.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[3],
        "payments.py",
        "sha3_blame",
        &[
            ("class PaymentService:", true),
            ("def __init__(self, db, stripe_client):", true),
            ("self.db = db", true),
            ("self.stripe = stripe_client", true),
            (
                "def charge(self, order_id, amount_cents, card_token):",
                true,
            ),
            ("stripe.charge.create", true),
            ("INSERT INTO payments", true),
            ("return result", true),
            ("def refund(self, payment_id):", true),
            ("stripe.refund.create", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "users.py",
        "chain3_prior_users_py",
        &[
            ("class UserService:", true),
            ("def get_user(self, user_id):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "products.py",
        "chain3_prior_products_py",
        &[
            ("class ProductService:", true),
            ("def list_products(self, category=None):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "orders.py",
        "chain3_prior_orders_py",
        &[
            ("class OrderService:", true),
            ("def create_order(self, user_id, items):", true),
        ],
    );

    // sha4 = C5': webhooks.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["webhooks.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "webhooks.py",
        "sha4_blame",
        &[
            ("class WebhookService:", true),
            ("def __init__(self, db, http_client):", true),
            ("self.db = db", true),
            ("self.http = http_client", true),
            ("def register(self, url, events):", true),
            ("INSERT INTO webhooks", true),
            ("def dispatch(self, event, payload):", true),
            ("SELECT * FROM webhooks", true),
            ("for hook in hooks:", true),
            ("self.http.post", true),
        ],
    );
    // Verify C1's file (users.py) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "users.py",
        "sha4_users_preserved",
        &[
            ("class UserService:", true),
            ("def get_user(self, user_id):", true),
            ("def create_user(self, name, email):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "products.py",
        "chain4_prior_products_py",
        &[
            ("class ProductService:", true),
            ("def list_products(self, category=None):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "orders.py",
        "chain4_prior_orders_py",
        &[
            ("class OrderService:", true),
            ("def create_order(self, user_id, items):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "payments.py",
        "chain4_prior_payments_py",
        &[
            ("class PaymentService:", true),
            (
                "def charge(self, order_id, amount_cents, card_token):",
                true,
            ),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_fast_path_python_microservice_5_endpoints,);
