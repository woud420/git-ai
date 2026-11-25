use super::*;

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

#[test]
fn test_fast_path_typescript_frontend_5_components() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("src/index.ts");
    init.set_contents(crate::lines!["// TypeScript frontend entry point"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a React component ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: Button.tsx
    let mut f1 = repo.filename("Button.tsx");
    f1.set_contents(crate::lines![
        "interface ButtonProps {".ai(),
        "  label: string;".ai(),
        "  onClick: () => void;".ai(),
        "  disabled?: boolean;".ai(),
        "  variant?: 'primary' | 'secondary' | 'danger';".ai(),
        "}".ai(),
        "export function Button({ label, onClick, disabled = false, variant = 'primary' }: ButtonProps) {".ai(),
        "  const cls = `btn btn-${variant}${disabled ? ' btn-disabled' : ''}`;".ai(),
        "  return <button className={cls} onClick={onClick} disabled={disabled}>{label}</button>;".ai(),
        "}".ai(),
        "export default Button;".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Button component")
        .unwrap();

    // C2: Input.tsx
    let mut f2 = repo.filename("Input.tsx");
    f2.set_contents(crate::lines![
        "interface InputProps {".ai(),
        "  value: string;".ai(),
        "  onChange: (v: string) => void;".ai(),
        "  placeholder?: string;".ai(),
        "  type?: 'text' | 'email' | 'password';".ai(),
        "  error?: string;".ai(),
        "}".ai(),
        "export function Input({ value, onChange, placeholder, type = 'text', error }: InputProps) {".ai(),
        "  return (".ai(),
        "    <div className=\"input-wrapper\">".ai(),
        "      <input type={type} value={value} placeholder={placeholder} onChange={e => onChange(e.target.value)} />".ai(),
        "      {error && <span className=\"input-error\">{error}</span>}".ai(),
        "    </div>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Input component")
        .unwrap();

    // C3: Modal.tsx
    let mut f3 = repo.filename("Modal.tsx");
    f3.set_contents(crate::lines![
        "interface ModalProps {".ai(),
        "  isOpen: boolean;".ai(),
        "  onClose: () => void;".ai(),
        "  title: string;".ai(),
        "  children: React.ReactNode;".ai(),
        "}".ai(),
        "export function Modal({ isOpen, onClose, title, children }: ModalProps) {".ai(),
        "  if (!isOpen) return null;".ai(),
        "  return (".ai(),
        "    <div className=\"modal-overlay\" onClick={onClose}>".ai(),
        "      <div className=\"modal-content\" onClick={e => e.stopPropagation()}>".ai(),
        "        <h2>{title}</h2>".ai(),
        "        <div className=\"modal-body\">{children}</div>".ai(),
        "      </div>".ai(),
        "    </div>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Modal component")
        .unwrap();

    // C4: Table.tsx
    let mut f4 = repo.filename("Table.tsx");
    f4.set_contents(crate::lines![
        "interface Column<T> { key: keyof T; header: string; }".ai(),
        "interface TableProps<T> {".ai(),
        "  columns: Column<T>[];".ai(),
        "  data: T[];".ai(),
        "  onRowClick?: (row: T) => void;".ai(),
        "}".ai(),
        "export function Table<T extends { id: string | number }>({ columns, data, onRowClick }: TableProps<T>) {".ai(),
        "  return (".ai(),
        "    <table className=\"data-table\">".ai(),
        "      <thead><tr>{columns.map(c => <th key={String(c.key)}>{c.header}</th>)}</tr></thead>".ai(),
        "      <tbody>{data.map(row => <tr key={row.id} onClick={() => onRowClick?.(row)}>{columns.map(c => <td key={String(c.key)}>{String(row[c.key])}</td>)}</tr>)}</tbody>".ai(),
        "    </table>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Table component")
        .unwrap();

    // C5: Form.tsx
    let mut f5 = repo.filename("Form.tsx");
    f5.set_contents(crate::lines![
        "interface FormField { name: string; label: string; type: string; required?: boolean; }".ai(),
        "interface FormProps {".ai(),
        "  fields: FormField[];".ai(),
        "  onSubmit: (data: Record<string, string>) => void;".ai(),
        "  submitLabel?: string;".ai(),
        "}".ai(),
        "export function Form({ fields, onSubmit, submitLabel = 'Submit' }: FormProps) {".ai(),
        "  const [values, setValues] = React.useState<Record<string, string>>({});".ai(),
        "  const handleSubmit = (e: React.FormEvent) => { e.preventDefault(); onSubmit(values); };".ai(),
        "  return (".ai(),
        "    <form onSubmit={handleSubmit}>".ai(),
        "      {fields.map(f => <label key={f.name}>{f.label}<input name={f.name} type={f.type} required={f.required} onChange={e => setValues(v => ({...v, [f.name]: e.target.value}))} /></label>)}".ai(),
        "      <button type=\"submit\">{submitLabel}</button>".ai(),
        "    </form>".ai(),
        "  );".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add Form component")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on config files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "vite.config.ts",
        "import { defineConfig } from 'vite';\nexport default defineConfig({ plugins: [] });\n",
        "build: add vite config",
    );
    repo.commit_untracked_file(
        ".eslintrc.json",
        "{\"extends\": [\"eslint:recommended\", \"plugin:@typescript-eslint/recommended\"]}\n",
        "lint: add eslint config",
    );
    repo.commit_untracked_file("tsconfig.json",
        "{\"compilerOptions\": {\"target\": \"ES2020\", \"module\": \"ESNext\", \"jsx\": \"react-jsx\", \"strict\": true}}\n",
        "build: add tsconfig",
    );
    repo.commit_untracked_file("package.json",
        "{\"name\": \"frontend\", \"version\": \"1.0.0\", \"scripts\": {\"dev\": \"vite\", \"build\": \"vite build\"}}\n",
        "build: add package.json",
    );
    repo.commit_untracked_file("tailwind.config.js",
        "module.exports = { content: ['./src/**/*.{ts,tsx}'], theme: { extend: {} }, plugins: [] };\n",
        "style: add tailwind config",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only Button.tsx
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["Button.tsx"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &["Input.tsx", "Modal.tsx", "Table.tsx", "Form.tsx"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "Button.tsx",
        "sha0_blame",
        &[
            ("interface ButtonProps {", true),
            ("label: string;", true),
            ("onClick: () => void;", true),
            ("disabled?: boolean;", true),
            ("variant?: 'primary' | 'secondary' | 'danger';", true),
            ("}", true),
            ("export function Button", true),
            ("const cls =", true),
            ("return <button", true),
            ("}", true),
            ("export default Button;", true),
        ],
    );

    // sha1 = C2': Input.tsx
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["Input.tsx"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["Modal.tsx", "Table.tsx", "Form.tsx"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "Button.tsx",
        "chain1_prior_button_tsx",
        &[
            ("interface ButtonProps {", true),
            ("export function Button", true),
        ],
    );

    // sha2 = C3': Modal.tsx
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["Modal.tsx"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["Table.tsx", "Form.tsx"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "Button.tsx",
        "chain2_prior_button_tsx",
        &[
            ("interface ButtonProps {", true),
            ("export function Button", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "Input.tsx",
        "chain2_prior_input_tsx",
        &[
            ("interface InputProps {", true),
            ("export function Input", true),
        ],
    );

    // sha3 = C4': Table.tsx
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["Table.tsx"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["Form.tsx"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "Button.tsx",
        "chain3_prior_button_tsx",
        &[
            ("interface ButtonProps {", true),
            ("export function Button", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "Input.tsx",
        "chain3_prior_input_tsx",
        &[
            ("interface InputProps {", true),
            ("export function Input", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "Modal.tsx",
        "chain3_prior_modal_tsx",
        &[
            ("interface ModalProps {", true),
            ("export function Modal", true),
        ],
    );

    // sha4 = C5': Form.tsx
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["Form.tsx"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "Form.tsx",
        "sha4_blame",
        &[
            ("interface FormField", true),
            ("interface FormProps {", true),
            ("fields: FormField[];", true),
            ("onSubmit: (data: Record<string, string>) => void;", true),
            ("submitLabel?: string;", true),
            ("}", true),
            ("export function Form", true),
            ("const [values, setValues]", true),
            ("const handleSubmit", true),
            ("return (", true),
            ("<form onSubmit={handleSubmit}>", true),
            ("fields.map", true),
            ("<button type=\"submit\">", true),
            ("</form>", true),
            (");", true),
            ("}", true),
        ],
    );
    // Verify C1's file (Button.tsx) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "Button.tsx",
        "sha4_button_preserved",
        &[
            ("interface ButtonProps {", true),
            ("export function Button", true),
            ("export default Button;", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "Input.tsx",
        "chain4_prior_input_tsx",
        &[
            ("interface InputProps {", true),
            ("export function Input", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "Modal.tsx",
        "chain4_prior_modal_tsx",
        &[
            ("interface ModalProps {", true),
            ("export function Modal", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "Table.tsx",
        "chain4_prior_table_tsx",
        &[
            ("interface TableProps<T> {", true),
            ("export function Table<T", true),
        ],
    );
}

#[test]
fn test_fast_path_rust_library_5_modules() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("src/lib.rs");
    init.set_contents(crate::lines!["// Rust library crate root"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a new Rust module ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: src/parser.rs
    let mut f1 = repo.filename("src/parser.rs");
    f1.set_contents(crate::lines![
        "pub struct Parser {".ai(),
        "    input: String,".ai(),
        "    pos: usize,".ai(),
        "}".ai(),
        "impl Parser {".ai(),
        "    pub fn new(input: &str) -> Self {".ai(),
        "        Self { input: input.to_string(), pos: 0 }".ai(),
        "    }".ai(),
        "    pub fn parse_token(&mut self) -> Option<&str> {".ai(),
        "        let start = self.pos;".ai(),
        "        while self.pos < self.input.len() && !self.input.as_bytes()[self.pos].is_ascii_whitespace() { self.pos += 1; }".ai(),
        "        if start == self.pos { None } else { Some(&self.input[start..self.pos]) }".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add parser module")
        .unwrap();

    // C2: src/validator.rs
    let mut f2 = repo.filename("src/validator.rs");
    f2.set_contents(crate::lines![
        "pub struct Validator {".ai(),
        "    rules: Vec<Box<dyn Fn(&str) -> bool>>,".ai(),
        "}".ai(),
        "impl Validator {".ai(),
        "    pub fn new() -> Self {".ai(),
        "        Self { rules: Vec::new() }".ai(),
        "    }".ai(),
        "    pub fn add_rule(&mut self, rule: impl Fn(&str) -> bool + 'static) {".ai(),
        "        self.rules.push(Box::new(rule));".ai(),
        "    }".ai(),
        "    pub fn validate(&self, input: &str) -> bool {".ai(),
        "        self.rules.iter().all(|r| r(input))".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add validator module")
        .unwrap();

    // C3: src/formatter.rs
    let mut f3 = repo.filename("src/formatter.rs");
    f3.set_contents(crate::lines![
        "pub struct Formatter {".ai(),
        "    indent: usize,".ai(),
        "    style: FormatterStyle,".ai(),
        "}".ai(),
        "pub enum FormatterStyle { Compact, Pretty }".ai(),
        "impl Formatter {".ai(),
        "    pub fn new(indent: usize, style: FormatterStyle) -> Self {".ai(),
        "        Self { indent, style }".ai(),
        "    }".ai(),
        "    pub fn format(&self, tokens: &[&str]) -> String {".ai(),
        "        let sep = match self.style { FormatterStyle::Compact => \"\", FormatterStyle::Pretty => \"\\n\" };".ai(),
        "        tokens.join(sep)".ai(),
        "    }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add formatter module")
        .unwrap();

    // C4: src/encoder.rs
    let mut f4 = repo.filename("src/encoder.rs");
    f4.set_contents(crate::lines![
        "pub struct Encoder {".ai(),
        "    buffer: Vec<u8>,".ai(),
        "}".ai(),
        "impl Encoder {".ai(),
        "    pub fn new() -> Self {".ai(),
        "        Self { buffer: Vec::new() }".ai(),
        "    }".ai(),
        "    pub fn encode_str(&mut self, s: &str) {".ai(),
        "        let len = s.len() as u32;".ai(),
        "        self.buffer.extend_from_slice(&len.to_le_bytes());".ai(),
        "        self.buffer.extend_from_slice(s.as_bytes());".ai(),
        "    }".ai(),
        "    pub fn finish(self) -> Vec<u8> { self.buffer }".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add encoder module")
        .unwrap();

    // C5: src/decoder.rs
    let mut f5 = repo.filename("src/decoder.rs");
    f5.set_contents(crate::lines![
        "pub struct Decoder<'a> {".ai(),
        "    data: &'a [u8],".ai(),
        "    pos: usize,".ai(),
        "}".ai(),
        "impl<'a> Decoder<'a> {".ai(),
        "    pub fn new(data: &'a [u8]) -> Self {".ai(),
        "        Self { data, pos: 0 }".ai(),
        "    }".ai(),
        "    pub fn decode_str(&mut self) -> Option<&'a str> {".ai(),
        "        if self.pos + 4 > self.data.len() { return None; }".ai(),
        "        let len = u32::from_le_bytes(self.data[self.pos..self.pos+4].try_into().ok()?) as usize;".ai(),
        "        self.pos += 4;".ai(),
        "        let s = std::str::from_utf8(&self.data[self.pos..self.pos+len]).ok()?;".ai(),
        "        self.pos += len; Some(s)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add decoder module")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "Cargo.toml",
        "[package]\nname = \"mylib\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        "build: add Cargo.toml",
    );
    repo.commit_untracked_file(
        "build.rs",
        "fn main() { println!(\"cargo:rerun-if-changed=build.rs\"); }\n",
        "build: add build script",
    );
    repo.commit_untracked_file("benches/bench.rs",
        "use criterion::{criterion_group, criterion_main, Criterion};\nfn bench(_c: &mut Criterion) {}\ncriterion_group!(benches, bench);\ncriterion_main!(benches);\n",
        "bench: add criterion benchmark stub",
    );
    repo.commit_untracked_file(
        "examples/demo.rs",
        "fn main() { println!(\"demo\"); }\n",
        "examples: add demo",
    );
    repo.commit_untracked_file(
        "tests/integration.rs",
        "#[test]\nfn integration_placeholder() { assert!(true); }\n",
        "test: add integration test placeholder",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only src/parser.rs
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["src/parser.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "src/validator.rs",
            "src/formatter.rs",
            "src/encoder.rs",
            "src/decoder.rs",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "src/parser.rs",
        "sha0_blame",
        &[
            ("pub struct Parser {", true),
            ("input: String,", true),
            ("pos: usize,", true),
            ("}", true),
            ("impl Parser {", true),
            ("pub fn new(input: &str) -> Self {", true),
            ("Self { input: input.to_string(), pos: 0 }", true),
            ("}", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
            ("let start = self.pos;", true),
            ("while self.pos < self.input.len()", true),
            ("if start == self.pos", true),
            ("}", true),
            ("}", true),
        ],
    );

    // sha1 = C2': validator
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["src/validator.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["src/formatter.rs", "src/encoder.rs", "src/decoder.rs"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "src/validator.rs",
        "sha1_blame",
        &[
            ("pub struct Validator {", true),
            ("rules: Vec<Box<dyn Fn(&str) -> bool>>,", true),
            ("}", true),
            ("impl Validator {", true),
            ("pub fn new() -> Self {", true),
            ("Self { rules: Vec::new() }", true),
            ("}", true),
            (
                "pub fn add_rule(&mut self, rule: impl Fn(&str) -> bool + 'static) {",
                true,
            ),
            ("self.rules.push(Box::new(rule));", true),
            ("}", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
            ("self.rules.iter().all(|r| r(input))", true),
            ("}", true),
            ("}", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/parser.rs",
        "chain1_prior_parser_rs",
        &[
            ("pub struct Parser {", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
        ],
    );

    // sha2 = C3': formatter
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/formatter.rs"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["src/encoder.rs", "src/decoder.rs"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/parser.rs",
        "chain2_prior_parser_rs",
        &[
            ("pub struct Parser {", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/validator.rs",
        "chain2_prior_validator_rs",
        &[
            ("pub struct Validator {", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
        ],
    );

    // sha3 = C4': encoder
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["src/encoder.rs"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["src/decoder.rs"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/parser.rs",
        "chain3_prior_parser_rs",
        &[
            ("pub struct Parser {", true),
            ("pub fn parse_token(&mut self) -> Option<&str> {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/validator.rs",
        "chain3_prior_validator_rs",
        &[
            ("pub struct Validator {", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/formatter.rs",
        "chain3_prior_formatter_rs",
        &[
            ("pub struct Formatter {", true),
            ("pub fn format(&self, tokens: &[&str]) -> String {", true),
        ],
    );

    // sha4 = C5': decoder
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["src/decoder.rs"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "src/decoder.rs",
        "sha4_blame",
        &[
            ("pub struct Decoder<'a> {", true),
            ("data: &'a [u8],", true),
            ("pos: usize,", true),
            ("}", true),
            ("impl<'a> Decoder<'a> {", true),
            ("pub fn new(data: &'a [u8]) -> Self {", true),
            ("Self { data, pos: 0 }", true),
            ("}", true),
            ("pub fn decode_str(&mut self) -> Option<&'a str> {", true),
            ("if self.pos + 4 > self.data.len()", true),
            ("let len = u32::from_le_bytes", true),
            ("self.pos += 4;", true),
            ("let s = std::str::from_utf8", true),
            ("self.pos += len; Some(s)", true),
            ("}", true),
        ],
    );
    // Verify C1's file (src/parser.rs) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/parser.rs",
        "sha4_parser_preserved",
        &[
            ("pub struct Parser {", true),
            ("pub fn new(input: &str) -> Self {", true),
            ("pub fn parse_token(&mut self)", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/validator.rs",
        "chain4_prior_validator_rs",
        &[
            ("pub struct Validator {", true),
            ("pub fn validate(&self, input: &str) -> bool {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/formatter.rs",
        "chain4_prior_formatter_rs",
        &[
            ("pub struct Formatter {", true),
            ("pub fn format(&self, tokens: &[&str]) -> String {", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/encoder.rs",
        "chain4_prior_encoder_rs",
        &[
            ("pub struct Encoder {", true),
            ("pub fn encode_str(&mut self, s: &str) {", true),
        ],
    );
}

#[test]
fn test_fast_path_go_service_5_handlers() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("main.go");
    init.set_contents(crate::lines!["// Go HTTP service"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits, each adding a Go handler file ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: handlers/user.go
    let mut f1 = repo.filename("handlers/user.go");
    f1.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import \"net/http\"".ai(),
        "".ai(),
        "type UserHandler struct { store UserStore }".ai(),
        "".ai(),
        "func NewUserHandler(s UserStore) *UserHandler { return &UserHandler{store: s} }".ai(),
        "".ai(),
        "func (h *UserHandler) GetUser(w http.ResponseWriter, r *http.Request) {".ai(),
        "    id := r.PathValue(\"id\")".ai(),
        "    user, err := h.store.Find(id)".ai(),
        "    if err != nil { http.Error(w, err.Error(), http.StatusNotFound); return }".ai(),
        "    writeJSON(w, user)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add user handler").unwrap();

    // C2: handlers/product.go
    let mut f2 = repo.filename("handlers/product.go");
    f2.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import \"net/http\"".ai(),
        "".ai(),
        "type ProductHandler struct { store ProductStore }".ai(),
        "".ai(),
        "func NewProductHandler(s ProductStore) *ProductHandler { return &ProductHandler{store: s} }".ai(),
        "".ai(),
        "func (h *ProductHandler) ListProducts(w http.ResponseWriter, r *http.Request) {".ai(),
        "    products, err := h.store.List()".ai(),
        "    if err != nil { http.Error(w, err.Error(), http.StatusInternalServerError); return }".ai(),
        "    writeJSON(w, products)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add product handler")
        .unwrap();

    // C3: handlers/order.go
    let mut f3 = repo.filename("handlers/order.go");
    f3.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import (\"net/http\"; \"encoding/json\")".ai(),
        "".ai(),
        "type OrderHandler struct { store OrderStore }".ai(),
        "".ai(),
        "func NewOrderHandler(s OrderStore) *OrderHandler { return &OrderHandler{store: s} }".ai(),
        "".ai(),
        "func (h *OrderHandler) CreateOrder(w http.ResponseWriter, r *http.Request) {".ai(),
        "    var req CreateOrderRequest".ai(),
        "    if err := json.NewDecoder(r.Body).Decode(&req); err != nil { http.Error(w, err.Error(), http.StatusBadRequest); return }".ai(),
        "    order, err := h.store.Create(req)".ai(),
        "    if err != nil { http.Error(w, err.Error(), http.StatusInternalServerError); return }".ai(),
        "    writeJSON(w, order)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add order handler")
        .unwrap();

    // C4: handlers/auth.go
    let mut f4 = repo.filename("handlers/auth.go");
    f4.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import (\"net/http\"; \"time\")".ai(),
        "".ai(),
        "type AuthHandler struct { svc AuthService }".ai(),
        "".ai(),
        "func NewAuthHandler(s AuthService) *AuthHandler { return &AuthHandler{svc: s} }".ai(),
        "".ai(),
        "func (h *AuthHandler) Login(w http.ResponseWriter, r *http.Request) {".ai(),
        "    token, err := h.svc.Authenticate(r.FormValue(\"user\"), r.FormValue(\"pass\"))".ai(),
        "    if err != nil { http.Error(w, \"unauthorized\", http.StatusUnauthorized); return }".ai(),
        "    http.SetCookie(w, &http.Cookie{Name: \"session\", Value: token, Expires: time.Now().Add(24*time.Hour)})".ai(),
        "    w.WriteHeader(http.StatusOK)".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add auth handler").unwrap();

    // C5: handlers/health.go
    let mut f5 = repo.filename("handlers/health.go");
    f5.set_contents(crate::lines![
        "package handlers".ai(),
        "".ai(),
        "import (\"net/http\"; \"encoding/json\")".ai(),
        "".ai(),
        "type HealthHandler struct { version string }".ai(),
        "".ai(),
        "func NewHealthHandler(v string) *HealthHandler { return &HealthHandler{version: v} }".ai(),
        "".ai(),
        "func (h *HealthHandler) Health(w http.ResponseWriter, r *http.Request) {".ai(),
        "    json.NewEncoder(w).Encode(map[string]string{\"status\": \"ok\", \"version\": h.version})".ai(),
        "}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add health handler")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "go.mod",
        "module example.com/service\n\ngo 1.21\n",
        "build: add go.mod",
    );
    repo.commit_untracked_file(
        "cmd/main.go",
        "package main\n\nfunc main() {}\n",
        "build: add cmd/main.go",
    );
    repo.commit_untracked_file("Dockerfile",
        "FROM golang:1.21\nWORKDIR /app\nCOPY . .\nRUN go build -o server cmd/main.go\nCMD [\"./server\"]\n",
        "build: add Dockerfile",
    );
    repo.commit_untracked_file(
        "docker-compose.yml",
        "version: '3.8'\nservices:\n  app:\n    build: .\n    ports:\n      - '8080:8080'\n",
        "build: add docker-compose.yml",
    );
    repo.commit_untracked_file(
        "Makefile",
        "build:\n\tgo build ./...\ntest:\n\tgo test ./...\n.PHONY: build test\n",
        "build: add Makefile",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only handlers/user.go
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["handlers/user.go"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "handlers/product.go",
            "handlers/order.go",
            "handlers/auth.go",
            "handlers/health.go",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "handlers/user.go",
        "sha0_blame",
        &[
            ("package handlers", true),
            ("", true),
            ("import \"net/http\"", true),
            ("", true),
            ("type UserHandler struct", true),
            ("", true),
            ("func NewUserHandler", true),
            ("", true),
            ("func (h *UserHandler) GetUser", true),
            ("id := r.PathValue", true),
            ("user, err := h.store.Find", true),
            ("if err != nil", true),
            ("writeJSON(w, user)", true),
            ("}", true),
        ],
    );

    // sha1 = C2': product
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["handlers/product.go"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &[
            "handlers/order.go",
            "handlers/auth.go",
            "handlers/health.go",
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "handlers/user.go",
        "chain1_prior_user_go",
        &[
            ("type UserHandler struct", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );

    // sha2 = C3': order
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["handlers/order.go"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["handlers/auth.go", "handlers/health.go"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "handlers/user.go",
        "chain2_prior_user_go",
        &[
            ("type UserHandler struct", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "handlers/product.go",
        "chain2_prior_product_go",
        &[
            ("type ProductHandler struct", true),
            ("func (h *ProductHandler) ListProducts", true),
        ],
    );

    // sha3 = C4': auth
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["handlers/auth.go"]);
    assert_note_no_forbidden_files(&repo, &chain[3], "sha3_no_future", &["handlers/health.go"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "handlers/user.go",
        "chain3_prior_user_go",
        &[
            ("type UserHandler struct", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "handlers/product.go",
        "chain3_prior_product_go",
        &[
            ("type ProductHandler struct", true),
            ("func (h *ProductHandler) ListProducts", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "handlers/order.go",
        "chain3_prior_order_go",
        &[
            ("type OrderHandler struct", true),
            ("func (h *OrderHandler) CreateOrder", true),
        ],
    );

    // sha4 = C5': health
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["handlers/health.go"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "handlers/health.go",
        "sha4_blame",
        &[
            ("package handlers", true),
            ("", true),
            ("import", true),
            ("", true),
            ("type HealthHandler struct", true),
            ("", true),
            ("func NewHealthHandler", true),
            ("", true),
            ("func (h *HealthHandler) Health", true),
            ("json.NewEncoder(w).Encode", true),
            ("}", true),
        ],
    );
    // Verify C1's file (handlers/user.go) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/user.go",
        "sha4_user_preserved",
        &[
            ("type UserHandler struct", true),
            ("func NewUserHandler", true),
            ("func (h *UserHandler) GetUser", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/product.go",
        "chain4_prior_product_go",
        &[
            ("type ProductHandler struct", true),
            ("func (h *ProductHandler) ListProducts", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/order.go",
        "chain4_prior_order_go",
        &[
            ("type OrderHandler struct", true),
            ("func (h *OrderHandler) CreateOrder", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "handlers/auth.go",
        "chain4_prior_auth_go",
        &[
            ("type AuthHandler struct", true),
            ("func (h *AuthHandler) Login", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(
    test_fast_path_python_microservice_5_endpoints,
    test_fast_path_rust_library_5_modules,
    test_fast_path_typescript_frontend_5_components,
    test_fast_path_go_service_5_handlers,
);
