use super::*;

#[test]
fn test_fast_path_mixed_ai_and_human_feature_commits() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("app.py");
    init.set_contents(crate::lines!["# Python application"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits alternating AI/human ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: human-only commit — config.py (plain string, no .ai())
    repo.commit_untracked_file(
        "config.py",
        "DATABASE_URL = 'sqlite:///app.db'\nDEBUG = False\nSECRET_KEY = 'changeme'\n",
        "config: add app config",
    );

    // C2: AI commit — auth.py
    let mut f2 = repo.filename("auth.py");
    f2.set_contents(crate::lines![
        "import hashlib, secrets".ai(),
        "".ai(),
        "class AuthService:".ai(),
        "    def __init__(self, user_store):".ai(),
        "        self.user_store = user_store".ai(),
        "    def hash_password(self, password):".ai(),
        "        salt = secrets.token_hex(16)".ai(),
        "        hashed = hashlib.sha256((password + salt).encode()).hexdigest()".ai(),
        "        return f'{salt}:{hashed}'".ai(),
        "    def verify_password(self, password, stored):".ai(),
        "        salt, hashed = stored.split(':')".ai(),
        "        return hashlib.sha256((password + salt).encode()).hexdigest() == hashed".ai(),
    ]);
    repo.stage_all_and_commit("feat: add auth service").unwrap();

    // C3: AI commit — middleware.py
    let mut f3 = repo.filename("middleware.py");
    f3.set_contents(crate::lines![
        "from functools import wraps".ai(),
        "from flask import request, jsonify, g".ai(),
        "".ai(),
        "def require_auth(f):".ai(),
        "    @wraps(f)".ai(),
        "    def decorated(*args, **kwargs):".ai(),
        "        token = request.headers.get('Authorization', '').removeprefix('Bearer ')".ai(),
        "        if not token:".ai(),
        "            return jsonify({'error': 'missing token'}), 401".ai(),
        "        g.user = verify_token(token)".ai(),
        "        return f(*args, **kwargs)".ai(),
        "    return decorated".ai(),
    ]);
    repo.stage_all_and_commit("feat: add auth middleware")
        .unwrap();

    // C4: human-only commit — requirements.txt
    repo.commit_untracked_file(
        "requirements.txt",
        "flask==3.0.0\nsqlalchemy==2.0.23\nclick==8.1.7\n",
        "deps: add requirements.txt",
    );

    // C5: AI commit — router.py
    let mut f5 = repo.filename("router.py");
    f5.set_contents(crate::lines![
        "from flask import Blueprint, jsonify, request".ai(),
        "".ai(),
        "api = Blueprint('api', __name__, url_prefix='/api/v1')".ai(),
        "".ai(),
        "@api.route('/health')".ai(),
        "def health():".ai(),
        "    return jsonify({'status': 'ok'})".ai(),
        "".ai(),
        "@api.route('/users', methods=['GET'])".ai(),
        "def list_users():".ai(),
        "    return jsonify({'users': []})".ai(),
    ]);
    repo.stage_all_and_commit("feat: add API router").unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "tests/test_smoke.py",
        "def test_smoke(): assert True\n",
        "test: add smoke test",
    );
    repo.commit_untracked_file(".github/workflows/test.yml",
        "name: Test\non: [push]\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps: [{uses: actions/checkout@v3}, {run: pytest}]\n",
        "ci: add test workflow",
    );
    repo.commit_untracked_file(
        "pyproject.toml",
        "[build-system]\nrequires = ['setuptools']\nbuild-backend = 'setuptools.build_meta'\n",
        "build: add pyproject.toml",
    );
    repo.commit_untracked_file(
        ".gitignore",
        "__pycache__/\n*.pyc\n.env\nvenv/\n",
        "git: add .gitignore",
    );
    repo.commit_untracked_file(
        "README.rst",
        "Python App\n==========\n\nInstall and run the app.\n",
        "docs: add README",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': human commit (commit_untracked_file — no AI tracking, no note after rebase).
    // Verify no AI files leak in if a note somehow exists.
    assert_note_no_forbidden_files_if_present(
        &repo,
        &chain[0],
        "sha0_no_ai",
        &["auth.py", "middleware.py", "router.py"],
    );

    // sha1 = C2': note has auth.py only
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["auth.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["middleware.py", "router.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "auth.py",
        "sha1_blame",
        &[
            ("import hashlib, secrets", true),
            ("", true),
            ("class AuthService:", true),
            ("def __init__(self, user_store):", true),
            ("self.user_store = user_store", true),
            ("def hash_password(self, password):", true),
            ("salt = secrets.token_hex(16)", true),
            ("hashed = hashlib.sha256", true),
            ("return f'{salt}:{hashed}'", true),
            ("def verify_password(self, password, stored):", true),
            ("salt, hashed = stored.split(':')", true),
            ("return hashlib.sha256", true),
        ],
    );

    // sha2 = C3': note has middleware.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["middleware.py"]);
    assert_note_no_forbidden_files(&repo, &chain[2], "sha2_no_future", &["router.py"]);
    // Verify auth.py attribution still intact at this position (not wiped by C3 processing).
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "auth.py",
        "sha2_auth_preserved",
        &[("class AuthService:", true), ("def hash_password", true)],
    );

    // sha3 = C4': human commit (commit_untracked_file — no note expected).
    // Just verify no future AI file leaked into a note if one exists.
    assert_note_no_forbidden_files_if_present(&repo, &chain[3], "sha3_no_future", &["router.py"]);

    // sha4 = C5': note has router.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["router.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "router.py",
        "sha4_blame",
        &[
            ("from flask import Blueprint, jsonify, request", true),
            ("", true),
            (
                "api = Blueprint('api', __name__, url_prefix='/api/v1')",
                true,
            ),
            ("", true),
            ("@api.route('/health')", true),
            ("def health():", true),
            ("return jsonify({'status': 'ok'})", true),
            ("", true),
            ("@api.route('/users', methods=['GET'])", true),
            ("def list_users():", true),
            ("return jsonify({'users': []})", true),
        ],
    );
    // Verify auth.py (C2's file) still correctly attributed at tip — not corrupted by later commits.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "auth.py",
        "sha4_auth_preserved",
        &[
            ("class AuthService:", true),
            ("def hash_password", true),
            ("def verify_password", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "middleware.py",
        "chain4_prior_middleware_py",
        &[
            ("def require_auth(f):", true),
            ("def decorated(*args, **kwargs):", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_fast_path_mixed_ai_and_human_feature_commits,);
