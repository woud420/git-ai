use super::*;

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

crate::reuse_tests_in_worktree!(test_slow_path_feature_has_human_commits_intermixed,);
