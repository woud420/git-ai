use super::*;

const PRIOR_SAMPLES: &[PriorBlameSample] = &[
    (
        "src/api/endpoints.py",
        "endpoints.py",
        ["bp = Blueprint('api', __name__)", "def list_items():"],
    ),
    (
        "src/models/user.py",
        "user.py",
        ["class User:", "email: str"],
    ),
    (
        "src/services/auth.py",
        "auth.py",
        ["def create_token(user_id: int", "SECRET_KEY = 'dev-secret'"],
    ),
    (
        "src/repositories/user_repo.py",
        "user_repo.py",
        [
            "class UserRepository:",
            "def find_by_id(self, user_id: int)",
        ],
    ),
];

#[test]
fn test_fast_path_nested_directory_structure() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("src/__init__.py");
    init.set_contents(crate::lines!["# src package"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits adding files in nested directories ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: src/api/endpoints.py
    let mut f1 = repo.filename("src/api/endpoints.py");
    f1.set_contents(crate::lines![
        "from flask import Blueprint, jsonify, request".ai(),
        "".ai(),
        "bp = Blueprint('api', __name__)".ai(),
        "".ai(),
        "@bp.route('/items', methods=['GET'])".ai(),
        "def list_items():".ai(),
        "    page = request.args.get('page', 1, type=int)".ai(),
        "    per_page = request.args.get('per_page', 20, type=int)".ai(),
        "    items = Item.query.paginate(page=page, per_page=per_page)".ai(),
        "    return jsonify({'items': [i.to_dict() for i in items], 'total': items.total})".ai(),
    ]);
    repo.stage_all_and_commit("feat: add API endpoints")
        .unwrap();

    // C2: src/models/user.py
    let mut f2 = repo.filename("src/models/user.py");
    f2.set_contents(crate::lines![
        "from dataclasses import dataclass, field".ai(),
        "from datetime import datetime".ai(),
        "from typing import Optional".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    email: str".ai(),
        "    name: str".ai(),
        "    created_at: datetime = field(default_factory=datetime.utcnow)".ai(),
        "    is_active: bool = True".ai(),
        "    role: str = 'user'".ai(),
    ]);
    repo.stage_all_and_commit("feat: add User model").unwrap();

    // C3: src/services/auth.py
    let mut f3 = repo.filename("src/services/auth.py");
    f3.set_contents(crate::lines![
        "import jwt".ai(),
        "from datetime import datetime, timedelta".ai(),
        "from functools import wraps".ai(),
        "from flask import request, g".ai(),
        "".ai(),
        "SECRET_KEY = 'dev-secret'".ai(),
        "".ai(),
        "def create_token(user_id: int, expires_in: int = 3600) -> str:".ai(),
        "    payload = {'sub': user_id, 'exp': datetime.utcnow() + timedelta(seconds=expires_in)}"
            .ai(),
        "    return jwt.encode(payload, SECRET_KEY, algorithm='HS256')".ai(),
    ]);
    repo.stage_all_and_commit("feat: add auth service").unwrap();

    // C4: src/repositories/user_repo.py
    let mut f4 = repo.filename("src/repositories/user_repo.py");
    f4.set_contents(crate::lines![
        "from typing import Optional, List".ai(),
        "from src.models.user import User".ai(),
        "".ai(),
        "class UserRepository:".ai(),
        "    def __init__(self, session):".ai(),
        "        self.session = session".ai(),
        "    def find_by_id(self, user_id: int) -> Optional[User]:".ai(),
        "        return self.session.query(User).filter_by(id=user_id).first()".ai(),
        "    def find_by_email(self, email: str) -> Optional[User]:".ai(),
        "        return self.session.query(User).filter_by(email=email).first()".ai(),
        "    def list_active(self) -> List[User]:".ai(),
        "        return self.session.query(User).filter_by(is_active=True).all()".ai(),
    ]);
    repo.stage_all_and_commit("feat: add user repository")
        .unwrap();

    // C5: src/middleware/logging.py
    let mut f5 = repo.filename("src/middleware/logging.py");
    f5.set_contents(crate::lines![
        "import time, logging".ai(),
        "from flask import request, g".ai(),
        "".ai(),
        "logger = logging.getLogger(__name__)".ai(),
        "".ai(),
        "def log_requests(app):".ai(),
        "    @app.before_request".ai(),
        "    def before():".ai(),
        "        g.start_time = time.time()".ai(),
        "    @app.after_request".ai(),
        "    def after(response):".ai(),
        "        elapsed = (time.time() - g.start_time) * 1000".ai(),
        "        logger.info('%s %s %s %.1fms', request.method, request.path, response.status_code, elapsed)".ai(),
        "        return response".ai(),
    ]);
    repo.stage_all_and_commit("feat: add request logging middleware")
        .unwrap();

    // === MAIN BRANCH: 5 human commits in tests/, docs/, .github/, scripts/, . ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "tests/conftest.py",
        "import pytest\n\n@pytest.fixture\ndef client(app):\n    return app.test_client()\n",
        "test: add pytest conftest",
    );
    repo.commit_untracked_file(
        "docs/architecture.md",
        "# Architecture\n\nThis is a Flask-based API.\n",
        "docs: add architecture overview",
    );
    repo.commit_untracked_file(".github/workflows/lint.yml",
        "name: Lint\non: [push]\njobs:\n  lint:\n    runs-on: ubuntu-latest\n    steps: [{uses: actions/checkout@v3}, {run: flake8 src/}]\n",
        "ci: add lint workflow",
    );
    repo.commit_untracked_file(
        "scripts/seed_db.py",
        "#!/usr/bin/env python3\nprint('Seeding database...')\n",
        "scripts: add db seed script",
    );
    repo.commit_untracked_file(
        "alembic.ini",
        "[alembic]\nscript_location = migrations\nsqlalchemy.url = sqlite:///app.db\n",
        "db: add alembic config",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': only src/api/endpoints.py
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["src/api/endpoints.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "src/models/user.py",
            "src/services/auth.py",
            "src/repositories/user_repo.py",
            "src/middleware/logging.py",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "src/api/endpoints.py",
        "sha0_blame",
        &[
            ("from flask import Blueprint, jsonify, request", true),
            ("", true),
            ("bp = Blueprint('api', __name__)", true),
            ("", true),
            ("@bp.route('/items', methods=['GET'])", true),
            ("def list_items():", true),
            ("page = request.args.get('page'", true),
            ("per_page = request.args.get('per_page'", true),
            ("items = Item.query.paginate", true),
            ("return jsonify", true),
        ],
    );

    // sha1 = C2': src/models/user.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["src/models/user.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &[
            "src/services/auth.py",
            "src/repositories/user_repo.py",
            "src/middleware/logging.py",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[1],
        "src/models/user.py",
        "sha1_blame",
        &[
            ("from dataclasses import dataclass, field", true),
            ("from datetime import datetime", true),
            ("from typing import Optional", true),
            ("", true),
            ("@dataclass", true),
            ("class User:", true),
            ("id: int", true),
            ("email: str", true),
            ("name: str", true),
            ("created_at: datetime", true),
            ("is_active: bool = True", true),
            ("role: str = 'user'", true),
        ],
    );
    assert_prior_blame_samples(&repo, &chain[1], 1, &PRIOR_SAMPLES[0..1]);

    // sha2 = C3': src/services/auth.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/services/auth.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["src/repositories/user_repo.py", "src/middleware/logging.py"],
    );
    assert_prior_blame_samples(&repo, &chain[2], 2, &PRIOR_SAMPLES[0..2]);

    // sha3 = C4': src/repositories/user_repo.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(
        &repo,
        &chain[3],
        "sha3_files",
        &["src/repositories/user_repo.py"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[3],
        "sha3_no_future",
        &["src/middleware/logging.py"],
    );
    assert_prior_blame_samples(&repo, &chain[3], 3, &PRIOR_SAMPLES[0..3]);

    // sha4 = C5': src/middleware/logging.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["src/middleware/logging.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "src/middleware/logging.py",
        "sha4_blame",
        &[
            ("import time, logging", true),
            ("from flask import request, g", true),
            ("", true),
            ("logger = logging.getLogger(__name__)", true),
            ("", true),
            ("def log_requests(app):", true),
            ("@app.before_request", true),
            ("def before():", true),
            ("g.start_time = time.time()", true),
            ("@app.after_request", true),
            ("def after(response):", true),
            ("elapsed = (time.time()", true),
            ("logger.info", true),
            ("return response", true),
        ],
    );
    // Verify C1's file (src/api/endpoints.py) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/api/endpoints.py",
        "sha4_endpoints_preserved",
        &[
            ("bp = Blueprint('api', __name__)", true),
            ("def list_items():", true),
            ("return jsonify", true),
        ],
    );
    assert_prior_blame_samples(&repo, &chain[4], 4, &PRIOR_SAMPLES[1..4]);
}

crate::reuse_tests_in_worktree!(test_fast_path_nested_directory_structure,);
