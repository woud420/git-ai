use super::*;

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
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "src/api/endpoints.py",
        "chain1_prior_endpoints.py",
        &[
            ("bp = Blueprint('api', __name__)", true),
            ("def list_items():", true),
        ],
    );

    // sha2 = C3': src/services/auth.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["src/services/auth.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["src/repositories/user_repo.py", "src/middleware/logging.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/api/endpoints.py",
        "chain2_prior_endpoints.py",
        &[
            ("bp = Blueprint('api', __name__)", true),
            ("def list_items():", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "src/models/user.py",
        "chain2_prior_user.py",
        &[("class User:", true), ("email: str", true)],
    );

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
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/api/endpoints.py",
        "chain3_prior_endpoints.py",
        &[
            ("bp = Blueprint('api', __name__)", true),
            ("def list_items():", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/models/user.py",
        "chain3_prior_user.py",
        &[("class User:", true), ("email: str", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "src/services/auth.py",
        "chain3_prior_auth.py",
        &[
            ("def create_token(user_id: int", true),
            ("SECRET_KEY = 'dev-secret'", true),
        ],
    );

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
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/models/user.py",
        "chain4_prior_user.py",
        &[("class User:", true), ("email: str", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/services/auth.py",
        "chain4_prior_auth.py",
        &[
            ("def create_token(user_id: int", true),
            ("SECRET_KEY = 'dev-secret'", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "src/repositories/user_repo.py",
        "chain4_prior_user_repo.py",
        &[
            ("class UserRepository:", true),
            ("def find_by_id(self, user_id: int)", true),
        ],
    );
}

#[test]
fn test_fast_path_single_file_grows_across_commits() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("app_init.py");
    init.set_contents(crate::lines!["# Service application"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits all modifying the SAME file: service.py ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: creates service.py with 8 AI lines
    let mut svc = repo.filename("service.py");
    svc.set_contents(crate::lines![
        "class DataService:".ai(),
        "    def __init__(self, db, cache):".ai(),
        "        self.db = db".ai(),
        "        self.cache = cache".ai(),
        "    def fetch(self, key):".ai(),
        "        if v := self.cache.get(key): return v".ai(),
        "        return self.db.get(key)".ai(),
        "    def store(self, key, value): self.db.set(key, value); self.cache.set(key, value)".ai(),
    ]);
    repo.stage_all_and_commit("feat: initial DataService")
        .unwrap();

    // C2: appends 6 AI lines (2 more methods)
    svc.set_contents(crate::lines![
        "class DataService:".ai(),
        "    def __init__(self, db, cache):".ai(),
        "        self.db = db".ai(),
        "        self.cache = cache".ai(),
        "    def fetch(self, key):".ai(),
        "        if v := self.cache.get(key): return v".ai(),
        "        return self.db.get(key)".ai(),
        "    def store(self, key, value): self.db.set(key, value); self.cache.set(key, value)".ai(),
        "    def delete(self, key):".ai(),
        "        self.cache.delete(key)".ai(),
        "        self.db.delete(key)".ai(),
        "    def exists(self, key): return self.cache.has(key) or self.db.has(key)".ai(),
        "    def keys(self, prefix=''): return [k for k in self.db.list() if k.startswith(prefix)]"
            .ai(),
        "    def flush(self): self.cache.clear()".ai(),
    ]);
    repo.stage_all_and_commit("feat: add delete and exists methods")
        .unwrap();

    // C3: appends 6 AI lines (2 more methods)
    svc.set_contents(crate::lines![
        "class DataService:".ai(),
        "    def __init__(self, db, cache):".ai(),
        "        self.db = db".ai(),
        "        self.cache = cache".ai(),
        "    def fetch(self, key):".ai(),
        "        if v := self.cache.get(key): return v".ai(),
        "        return self.db.get(key)".ai(),
        "    def store(self, key, value): self.db.set(key, value); self.cache.set(key, value)".ai(),
        "    def delete(self, key):".ai(),
        "        self.cache.delete(key)".ai(),
        "        self.db.delete(key)".ai(),
        "    def exists(self, key): return self.cache.has(key) or self.db.has(key)".ai(),
        "    def keys(self, prefix=''): return [k for k in self.db.list() if k.startswith(prefix)]".ai(),
        "    def flush(self): self.cache.clear()".ai(),
        "    def fetch_many(self, keys): return {k: self.fetch(k) for k in keys}".ai(),
        "    def store_many(self, items):".ai(),
        "        for k, v in items.items(): self.store(k, v)".ai(),
        "    def invalidate(self, pattern): [self.cache.delete(k) for k in self.cache.keys() if pattern in k]".ai(),
        "    def ttl_set(self, key, value, ttl): self.db.set_ex(key, value, ttl); self.cache.set(key, value)".ai(),
        "    def size(self): return self.db.dbsize()".ai(),
    ]);
    repo.stage_all_and_commit("feat: add batch and TTL methods")
        .unwrap();

    // C4: appends 6 AI lines (2 more methods)
    svc.set_contents(crate::lines![
        "class DataService:".ai(),
        "    def __init__(self, db, cache):".ai(),
        "        self.db = db".ai(),
        "        self.cache = cache".ai(),
        "    def fetch(self, key):".ai(),
        "        if v := self.cache.get(key): return v".ai(),
        "        return self.db.get(key)".ai(),
        "    def store(self, key, value): self.db.set(key, value); self.cache.set(key, value)".ai(),
        "    def delete(self, key):".ai(),
        "        self.cache.delete(key)".ai(),
        "        self.db.delete(key)".ai(),
        "    def exists(self, key): return self.cache.has(key) or self.db.has(key)".ai(),
        "    def keys(self, prefix=''): return [k for k in self.db.list() if k.startswith(prefix)]".ai(),
        "    def flush(self): self.cache.clear()".ai(),
        "    def fetch_many(self, keys): return {k: self.fetch(k) for k in keys}".ai(),
        "    def store_many(self, items):".ai(),
        "        for k, v in items.items(): self.store(k, v)".ai(),
        "    def invalidate(self, pattern): [self.cache.delete(k) for k in self.cache.keys() if pattern in k]".ai(),
        "    def ttl_set(self, key, value, ttl): self.db.set_ex(key, value, ttl); self.cache.set(key, value)".ai(),
        "    def size(self): return self.db.dbsize()".ai(),
        "    def watch(self, key, callback): self.db.subscribe(key, callback)".ai(),
        "    def unwatch(self, key, callback): self.db.unsubscribe(key, callback)".ai(),
        "    def transaction(self, fn):".ai(),
        "        with self.db.pipeline() as pipe: fn(pipe); pipe.execute()".ai(),
        "    def backup(self, path): self.db.bgsave(); return self.db.dump(path)".ai(),
        "    def restore(self, path): self.db.restore_dump(path)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add watch, transaction, backup methods")
        .unwrap();

    // C5: appends 6 AI lines (final methods)
    svc.set_contents(crate::lines![
        "class DataService:".ai(),
        "    def __init__(self, db, cache):".ai(),
        "        self.db = db".ai(),
        "        self.cache = cache".ai(),
        "    def fetch(self, key):".ai(),
        "        if v := self.cache.get(key): return v".ai(),
        "        return self.db.get(key)".ai(),
        "    def store(self, key, value): self.db.set(key, value); self.cache.set(key, value)".ai(),
        "    def delete(self, key):".ai(),
        "        self.cache.delete(key)".ai(),
        "        self.db.delete(key)".ai(),
        "    def exists(self, key): return self.cache.has(key) or self.db.has(key)".ai(),
        "    def keys(self, prefix=''): return [k for k in self.db.list() if k.startswith(prefix)]".ai(),
        "    def flush(self): self.cache.clear()".ai(),
        "    def fetch_many(self, keys): return {k: self.fetch(k) for k in keys}".ai(),
        "    def store_many(self, items):".ai(),
        "        for k, v in items.items(): self.store(k, v)".ai(),
        "    def invalidate(self, pattern): [self.cache.delete(k) for k in self.cache.keys() if pattern in k]".ai(),
        "    def ttl_set(self, key, value, ttl): self.db.set_ex(key, value, ttl); self.cache.set(key, value)".ai(),
        "    def size(self): return self.db.dbsize()".ai(),
        "    def watch(self, key, callback): self.db.subscribe(key, callback)".ai(),
        "    def unwatch(self, key, callback): self.db.unsubscribe(key, callback)".ai(),
        "    def transaction(self, fn):".ai(),
        "        with self.db.pipeline() as pipe: fn(pipe); pipe.execute()".ai(),
        "    def backup(self, path): self.db.bgsave(); return self.db.dump(path)".ai(),
        "    def restore(self, path): self.db.restore_dump(path)".ai(),
        "    def stats(self): return self.db.info()".ai(),
        "    def ping(self): return self.db.ping()".ai(),
        "    def close(self):".ai(),
        "        self.cache.close()".ai(),
        "        self.db.close()".ai(),
        "    def __repr__(self): return f'DataService(db={self.db}, cache={self.cache})'".ai(),
    ]);
    repo.stage_all_and_commit("feat: add stats, ping, close methods")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "tests/test_service.py",
        "def test_placeholder(): pass\n",
        "test: add service test placeholder",
    );
    repo.commit_untracked_file("docker/Dockerfile.dev",
        "FROM python:3.11-slim\nWORKDIR /app\nCOPY requirements.txt .\nRUN pip install -r requirements.txt\n",
        "build: add dev Dockerfile",
    );
    repo.commit_untracked_file(
        ".env.example",
        "DATABASE_URL=redis://localhost:6379/0\nCACHE_URL=memcached://localhost:11211\n",
        "config: add .env.example",
    );
    repo.commit_untracked_file(
        "CHANGELOG.md",
        "# Changelog\n\n## [Unreleased]\n\n### Added\n- DataService class\n",
        "docs: add CHANGELOG",
    );
    repo.commit_untracked_file(
        "setup.py",
        "from setuptools import setup\nsetup(name='dataservice', version='0.1.0')\n",
        "build: add setup.py",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': service.py with ~8 AI lines (per-commit-delta: C1's lines only)
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["service.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "service.py",
        "sha0_blame",
        &[
            ("class DataService:", true),
            ("def __init__(self, db, cache):", true),
            ("self.db = db", true),
            ("self.cache = cache", true),
            ("def fetch(self, key):", true),
            ("if v := self.cache.get(key): return v", true),
            ("return self.db.get(key)", true),
            ("def store(self, key, value):", true),
        ],
    );

    // sha1 = C2': service.py (C2's delta only; fast-path remaps original note)
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["service.py"]);
    // C1 lines must still be AI-attributed at sha1.
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "service.py",
        "sha1_c1_preserved",
        &[
            ("class DataService:", true),
            ("def fetch(self, key):", true),
            ("def store(self, key, value):", true),
        ],
    );

    // sha2 = C3': service.py (C3's delta only; fast-path remaps original note)
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["service.py"]);
    // C2 lines must still be AI-attributed at sha2.
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "service.py",
        "sha2_c2_preserved",
        &[
            ("def delete(self, key):", true),
            ("def exists(self, key):", true),
            ("def flush(self):", true),
        ],
    );
    // C1 lines must also still be AI-attributed at sha2.
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "service.py",
        "chain2_prior_c1_service.py",
        &[
            ("class DataService:", true),
            ("def fetch(self, key):", true),
        ],
    );

    // sha3 = C4': service.py (C4's delta only; fast-path remaps original note)
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["service.py"]);
    // C3 lines must still be AI-attributed at sha3.
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "service.py",
        "sha3_c3_preserved",
        &[
            ("def fetch_many(self, keys):", true),
            ("def store_many(self, items):", true),
            ("def ttl_set(self, key, value, ttl):", true),
        ],
    );
    // C1 and C2 lines must also still be AI-attributed at sha3.
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "service.py",
        "chain3_prior_c1_service.py",
        &[
            ("class DataService:", true),
            ("def fetch(self, key):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "service.py",
        "chain3_prior_c2_service.py",
        &[
            ("def delete(self, key):", true),
            ("def exists(self, key):", true),
        ],
    );

    // sha4 = C5': service.py (C5's delta only; fast-path remaps original note)
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["service.py"]);
}

#[test]
fn test_fast_path_feature_deletes_file_then_recreates() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("pkg/__init__.py");
    init.set_contents(crate::lines!["# utilities package"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits with a file deletion in C3 ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: creates temp_module.py (AI, 8 lines) + util_a.py (AI, 6 lines)
    let mut temp = repo.filename("temp_module.py");
    temp.set_contents(crate::lines![
        "class TempProcessor:".ai(),
        "    def __init__(self, config):".ai(),
        "        self.config = config".ai(),
        "    def process(self, data):".ai(),
        "        return [self._transform(item) for item in data]".ai(),
        "    def _transform(self, item):".ai(),
        "        return {k: v for k, v in item.items() if k in self.config['fields']}".ai(),
        "    def flush(self): self.config.clear()".ai(),
    ]);
    let mut ua = repo.filename("util_a.py");
    ua.set_contents(crate::lines![
        "def parse_csv(path):".ai(),
        "    import csv".ai(),
        "    with open(path) as f:".ai(),
        "        return list(csv.DictReader(f))".ai(),
        "def write_csv(path, rows, fields):".ai(),
        "    import csv".ai(),
        "    with open(path, 'w', newline='') as f:".ai(),
        "        w = csv.DictWriter(f, fieldnames=fields); w.writeheader(); w.writerows(rows)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add temp_module and util_a")
        .unwrap();

    // C2: adds util_b.py (AI, 8 lines)
    let mut ub = repo.filename("util_b.py");
    ub.set_contents(crate::lines![
        "import json, pathlib".ai(),
        "".ai(),
        "def load_json(path):".ai(),
        "    return json.loads(pathlib.Path(path).read_text())".ai(),
        "".ai(),
        "def save_json(path, data, indent=2):".ai(),
        "    pathlib.Path(path).write_text(json.dumps(data, indent=indent))".ai(),
        "".ai(),
        "def merge_json_files(paths):".ai(),
        "    result = {}".ai(),
        "    for p in paths: result.update(load_json(p))".ai(),
        "    return result".ai(),
    ]);
    repo.stage_all_and_commit("feat: add util_b json utilities")
        .unwrap();

    // C3: DELETES temp_module.py (human commit) + adds util_c.py (AI, 6 lines)
    repo.git(&["rm", "temp_module.py"]).unwrap();
    // Stage the deletion and commit (bypassing git-ai to make it a plain human commit)
    repo.git_og(&["commit", "-m", "refactor: remove temp_module"])
        .unwrap();
    // Now add util_c.py via AI
    let mut uc = repo.filename("util_c.py");
    uc.set_contents(crate::lines![
        "import hashlib".ai(),
        "".ai(),
        "def md5(data): return hashlib.md5(data.encode()).hexdigest()".ai(),
        "def sha256(data): return hashlib.sha256(data.encode()).hexdigest()".ai(),
        "def sha512(data): return hashlib.sha512(data.encode()).hexdigest()".ai(),
        "def hmac_sign(key, data):".ai(),
        "    import hmac".ai(),
        "    return hmac.new(key.encode(), data.encode(), hashlib.sha256).hexdigest()".ai(),
    ]);
    repo.stage_all_and_commit("feat: add util_c crypto utilities")
        .unwrap();

    // C4: adds util_d.py (AI, 8 lines)
    let mut ud = repo.filename("util_d.py");
    ud.set_contents(crate::lines![
        "from typing import TypeVar, Callable, Any".ai(),
        "T = TypeVar('T')".ai(),
        "".ai(),
        "def retry(fn: Callable, attempts: int = 3, exceptions=(Exception,)):".ai(),
        "    for i in range(attempts):".ai(),
        "        try: return fn()".ai(),
        "        except exceptions:".ai(),
        "            if i == attempts - 1: raise".ai(),
        "".ai(),
        "def memoize(fn: Callable[..., T]) -> Callable[..., T]:".ai(),
        "    cache: dict[Any, T] = {}".ai(),
        "    def wrapper(*args): return cache.setdefault(args, fn(*args))".ai(),
        "    return wrapper".ai(),
    ]);
    repo.stage_all_and_commit("feat: add util_d retry and memoize")
        .unwrap();

    // C5: adds util_e.py (AI, 6 lines)
    let mut ue = repo.filename("util_e.py");
    ue.set_contents(crate::lines![
        "import time, functools".ai(),
        "".ai(),
        "def timed(fn):".ai(),
        "    @functools.wraps(fn)".ai(),
        "    def wrapper(*a, **kw):".ai(),
        "        t = time.perf_counter(); r = fn(*a, **kw)".ai(),
        "        print(f'{fn.__name__} took {time.perf_counter()-t:.4f}s')".ai(),
        "        return r".ai(),
        "    return wrapper".ai(),
    ]);
    repo.stage_all_and_commit("feat: add util_e timing utilities")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "tests/test_utils.py",
        "def test_placeholder(): pass\n",
        "test: add utils test placeholder",
    );
    repo.commit_untracked_file(
        "pyproject.toml",
        "[build-system]\nrequires=['setuptools']\nbuild-backend='setuptools.build_meta'\n",
        "build: add pyproject.toml",
    );
    repo.commit_untracked_file(
        ".flake8",
        "[flake8]\nmax-line-length=120\nexclude=.git,__pycache__\n",
        "lint: add flake8 config",
    );
    repo.commit_untracked_file(
        "MANIFEST.in",
        "include *.py\ninclude *.md\n",
        "build: add MANIFEST.in",
    );
    repo.commit_untracked_file(
        "tox.ini",
        "[tox]\nenvlist = py311\n[testenv]\ncommands = pytest\n",
        "test: add tox config",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // The rebase of C3 was a human commit (git rm + git commit via git_og),
    // so the chain has 5 rebased commits from the feature branch.
    // However C3 was split: first the rm+commit then the util_c.py AI commit.
    // Actually let's count: C1, C2, C3_rm, C3_util_c, C4, C5 = 6 commits.
    // Wait — we committed the rm first (human) then util_c (AI) separately = 6 feature commits total.
    let chain = get_commit_chain(&repo, 6);
    // chain[0]=C1', chain[1]=C2', chain[2]=C3_rm', chain[3]=C3_util_c', chain[4]=C4', chain[5]=C5'

    // sha0 = C1': temp_module.py + util_a.py. Content-based mapping correctly
    // transfers attribution for both files since both exist identically at C1'.
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(
        &repo,
        &chain[0],
        "sha0_files",
        &["temp_module.py", "util_a.py"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &["util_b.py", "util_c.py", "util_d.py", "util_e.py"],
    );

    // sha1 = C2': util_b.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["util_b.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &["util_c.py", "util_d.py", "util_e.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "temp_module.py",
        "chain1_prior_temp_module.py",
        &[
            ("class TempProcessor:", true),
            ("def process(self, data):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "util_a.py",
        "chain1_prior_util_a.py",
        &[
            ("def parse_csv(path):", true),
            ("def write_csv(path, rows, fields):", true),
        ],
    );

    // sha2 = C3_rm': human deletion commit — no AI content so no note expected.
    assert_note_no_forbidden_files_if_present(
        &repo,
        &chain[2],
        "sha2_no_temp",
        &["temp_module.py"],
    );
    assert_note_no_forbidden_files_if_present(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["util_c.py", "util_d.py", "util_e.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "util_a.py",
        "chain2_prior_util_a.py",
        &[
            ("def parse_csv(path):", true),
            ("def write_csv(path, rows, fields):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "util_b.py",
        "chain2_prior_util_b.py",
        &[
            ("def load_json(path):", true),
            ("def save_json(path, data", true),
        ],
    );

    // sha3 = C3_util_c': util_c.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["util_c.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[3],
        "sha3_no_temp_or_future",
        &["temp_module.py", "util_d.py", "util_e.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "util_a.py",
        "chain3_prior_util_a.py",
        &[
            ("def parse_csv(path):", true),
            ("def write_csv(path, rows, fields):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "util_b.py",
        "chain3_prior_util_b.py",
        &[
            ("def load_json(path):", true),
            ("def save_json(path, data", true),
        ],
    );

    // sha4 = C4': util_d.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["util_d.py"]);
    assert_note_no_forbidden_files(&repo, &chain[4], "sha4_no_future", &["util_e.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "util_a.py",
        "chain4_prior_util_a.py",
        &[
            ("def parse_csv(path):", true),
            ("def write_csv(path, rows, fields):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "util_b.py",
        "chain4_prior_util_b.py",
        &[
            ("def load_json(path):", true),
            ("def save_json(path, data", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "util_c.py",
        "chain4_prior_util_c.py",
        &[("def md5(data):", true), ("def sha256(data):", true)],
    );

    // sha5 = C5': util_e.py
    assert_note_base_commit_matches(&repo, &chain[5], "sha5");
    assert_note_files_exact(&repo, &chain[5], "sha5_files", &["util_e.py"]);
    assert_blame_at_commit(
        &repo,
        &chain[5],
        "util_e.py",
        "sha5_blame",
        &[
            ("import time, functools", true),
            ("", true),
            ("def timed(fn):", true),
            ("@functools.wraps(fn)", true),
            ("def wrapper(*a, **kw):", true),
            ("t = time.perf_counter()", true),
            ("print(f'{fn.__name__}", true),
            ("return r", true),
            ("return wrapper", true),
        ],
    );
    // Verify C2's file (util_b.py) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[5],
        "util_b.py",
        "sha5_util_b_preserved",
        &[
            ("def load_json(path):", true),
            ("def save_json(path, data", true),
            ("def merge_json_files(paths):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[5],
        "util_a.py",
        "chain5_prior_util_a.py",
        &[
            ("def parse_csv(path):", true),
            ("def write_csv(path, rows, fields):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[5],
        "util_c.py",
        "chain5_prior_util_c.py",
        &[("def md5(data):", true), ("def sha256(data):", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[5],
        "util_d.py",
        "chain5_prior_util_d.py",
        &[
            ("def retry(fn: Callable", true),
            ("def memoize(fn: Callable", true),
        ],
    );

    // Note: accepted_lines is NOT monotonic here because chain[2] is a human deletion commit
    // (removes temp_module.py) which has 0 accepted lines, breaking the monotonic property.
}

#[test]
fn test_fast_path_multi_file_commits_2_files_each() {
    let repo = TestRepo::new();

    // Initial commit (shared base)
    let mut init = repo.filename("manage.py");
    init.set_contents(crate::lines!["# Django-style project management"]);
    repo.stage_all_and_commit("Initial commit").unwrap();
    let main_branch = repo.current_branch();

    // === FEATURE BRANCH: 5 commits each adding 2 AI files ===
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // C1: models.py (8 AI lines) + schemas.py (6 AI lines)
    let mut m1 = repo.filename("models.py");
    m1.set_contents(crate::lines![
        "from django.db import models".ai(),
        "".ai(),
        "class Product(models.Model):".ai(),
        "    name = models.CharField(max_length=200)".ai(),
        "    price = models.DecimalField(max_digits=10, decimal_places=2)".ai(),
        "    stock = models.IntegerField(default=0)".ai(),
        "    created_at = models.DateTimeField(auto_now_add=True)".ai(),
        "    class Meta: ordering = ['-created_at']".ai(),
    ]);
    let mut s1 = repo.filename("schemas.py");
    s1.set_contents(crate::lines![
        "from pydantic import BaseModel, condecimal".ai(),
        "from decimal import Decimal".ai(),
        "".ai(),
        "class ProductSchema(BaseModel):".ai(),
        "    name: str".ai(),
        "    price: condecimal(max_digits=10, decimal_places=2)".ai(),
        "    stock: int = 0".ai(),
        "    class Config: from_attributes = True".ai(),
    ]);
    repo.stage_all_and_commit("feat: add models and schemas")
        .unwrap();

    // C2: views.py (8 AI lines) + serializers.py (6 AI lines)
    let mut v2 = repo.filename("views.py");
    v2.set_contents(crate::lines![
        "from django.shortcuts import get_object_or_404".ai(),
        "from rest_framework.decorators import api_view".ai(),
        "from rest_framework.response import Response".ai(),
        "from .models import Product".ai(),
        "from .serializers import ProductSerializer".ai(),
        "".ai(),
        "@api_view(['GET'])".ai(),
        "def product_list(request): return Response(ProductSerializer(Product.objects.all(), many=True).data)".ai(),
    ]);
    let mut sz2 = repo.filename("serializers.py");
    sz2.set_contents(crate::lines![
        "from rest_framework import serializers".ai(),
        "from .models import Product".ai(),
        "".ai(),
        "class ProductSerializer(serializers.ModelSerializer):".ai(),
        "    class Meta:".ai(),
        "        model = Product".ai(),
        "        fields = ['id', 'name', 'price', 'stock', 'created_at']".ai(),
        "        read_only_fields = ['id', 'created_at']".ai(),
    ]);
    repo.stage_all_and_commit("feat: add views and serializers")
        .unwrap();

    // C3: urls.py (6 AI lines) + permissions.py (8 AI lines)
    let mut u3 = repo.filename("urls.py");
    u3.set_contents(crate::lines![
        "from django.urls import path".ai(),
        "from . import views".ai(),
        "".ai(),
        "app_name = 'shop'".ai(),
        "urlpatterns = [".ai(),
        "    path('products/', views.product_list, name='product-list'),".ai(),
        "    path('products/<int:pk>/', views.product_detail, name='product-detail'),".ai(),
        "]".ai(),
    ]);
    let mut p3 = repo.filename("permissions.py");
    p3.set_contents(crate::lines![
        "from rest_framework.permissions import BasePermission".ai(),
        "".ai(),
        "class IsOwnerOrReadOnly(BasePermission):".ai(),
        "    def has_object_permission(self, request, view, obj):".ai(),
        "        if request.method in ('GET', 'HEAD', 'OPTIONS'): return True".ai(),
        "        return obj.owner == request.user".ai(),
        "".ai(),
        "class IsStaff(BasePermission):".ai(),
        "    message = 'Staff access required.'".ai(),
        "    def has_permission(self, request, view): return bool(request.user and request.user.is_staff)".ai(),
    ]);
    repo.stage_all_and_commit("feat: add urls and permissions")
        .unwrap();

    // C4: signals.py (8 AI lines) + tasks.py (6 AI lines)
    let mut sg4 = repo.filename("signals.py");
    sg4.set_contents(crate::lines![
        "from django.db.models.signals import post_save, pre_delete".ai(),
        "from django.dispatch import receiver".ai(),
        "from .models import Product".ai(),
        "".ai(),
        "@receiver(post_save, sender=Product)".ai(),
        "def on_product_saved(sender, instance, created, **kwargs):".ai(),
        "    if created: print(f'New product created: {instance.name}')".ai(),
        "".ai(),
        "@receiver(pre_delete, sender=Product)".ai(),
        "def on_product_deleted(sender, instance, **kwargs):".ai(),
        "    print(f'Deleting product: {instance.name}')".ai(),
    ]);
    let mut t4 = repo.filename("tasks.py");
    t4.set_contents(crate::lines![
        "from celery import shared_task".ai(),
        "from .models import Product".ai(),
        "".ai(),
        "@shared_task".ai(),
        "def sync_inventory(product_id):".ai(),
        "    p = Product.objects.get(id=product_id)".ai(),
        "    # sync with external warehouse system".ai(),
        "    return {'product': p.name, 'stock': p.stock}".ai(),
    ]);
    repo.stage_all_and_commit("feat: add signals and tasks")
        .unwrap();

    // C5: middleware.py (8 AI lines) + decorators.py (6 AI lines)
    let mut mw5 = repo.filename("middleware.py");
    mw5.set_contents(crate::lines![
        "import time".ai(),
        "from django.utils.deprecation import MiddlewareMixin".ai(),
        "".ai(),
        "class RequestTimingMiddleware(MiddlewareMixin):".ai(),
        "    def process_request(self, request):".ai(),
        "        request._start_time = time.monotonic()".ai(),
        "    def process_response(self, request, response):".ai(),
        "        elapsed = (time.monotonic() - getattr(request, '_start_time', time.monotonic())) * 1000".ai(),
        "        response['X-Response-Time'] = f'{elapsed:.1f}ms'".ai(),
        "        return response".ai(),
    ]);
    let mut d5 = repo.filename("decorators.py");
    d5.set_contents(crate::lines![
        "from functools import wraps".ai(),
        "from django.http import JsonResponse".ai(),
        "".ai(),
        "def require_json(view_fn):".ai(),
        "    @wraps(view_fn)".ai(),
        "    def wrapper(request, *a, **kw):".ai(),
        "        if request.content_type != 'application/json': return JsonResponse({'error': 'JSON required'}, status=415)".ai(),
        "        return view_fn(request, *a, **kw)".ai(),
        "    return wrapper".ai(),
    ]);
    repo.stage_all_and_commit("feat: add middleware and decorators")
        .unwrap();

    // === MAIN BRANCH: 5 human commits on different files ===
    repo.git(&["checkout", &main_branch]).unwrap();
    repo.commit_untracked_file(
        "settings.py",
        "DEBUG = True\nINSTALLED_APPS = ['django.contrib.admin']\n",
        "config: add Django settings",
    );
    repo.commit_untracked_file(
        "requirements.txt",
        "django==4.2\ndjangorestframework==3.14\ncelery==5.3\npydantic==2.0\n",
        "deps: add requirements.txt",
    );
    repo.commit_untracked_file("Dockerfile",
        "FROM python:3.11\nWORKDIR /app\nCOPY requirements.txt .\nRUN pip install -r requirements.txt\n",
        "build: add Dockerfile",
    );
    repo.commit_untracked_file("docker-compose.yml",
        "version: '3.9'\nservices:\n  web:\n    build: .\n    ports: ['8000:8000']\n  worker:\n    build: .\n    command: celery -A app worker\n",
        "build: add docker-compose.yml",
    );
    repo.commit_untracked_file(
        ".env.example",
        "SECRET_KEY=change-me\nDEBUG=1\nDATABASE_URL=postgresql://localhost/app\n",
        "config: add .env.example",
    );

    // === REBASE feature onto main ===
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", &main_branch]).unwrap();

    // === VERIFY AT EVERY COMMIT ===
    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': {models.py, schemas.py}
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["models.py", "schemas.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[0],
        "sha0_no_future",
        &[
            "views.py",
            "serializers.py",
            "urls.py",
            "permissions.py",
            "signals.py",
            "tasks.py",
            "middleware.py",
            "decorators.py",
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "models.py",
        "sha0_blame_models",
        &[
            ("from django.db import models", true),
            ("", true),
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
            ("price = models.DecimalField", true),
            ("stock = models.IntegerField", true),
            ("created_at = models.DateTimeField", true),
            ("class Meta: ordering", true),
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "schemas.py",
        "sha0_blame_schemas",
        &[
            ("from pydantic import BaseModel, condecimal", true),
            ("from decimal import Decimal", true),
            ("", true),
            ("class ProductSchema(BaseModel):", true),
            ("name: str", true),
            ("price: condecimal", true),
            ("stock: int = 0", true),
            ("class Config: from_attributes = True", true),
        ],
    );

    // sha1 = C2': views.py + serializers.py
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(
        &repo,
        &chain[1],
        "sha1_files",
        &["views.py", "serializers.py"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[1],
        "sha1_no_future",
        &[
            "urls.py",
            "permissions.py",
            "signals.py",
            "tasks.py",
            "middleware.py",
            "decorators.py",
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "models.py",
        "chain1_prior_models.py",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "schemas.py",
        "chain1_prior_schemas.py",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
        ],
    );

    // sha2 = C3': urls.py + permissions.py
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(
        &repo,
        &chain[2],
        "sha2_files",
        &["urls.py", "permissions.py"],
    );
    assert_note_no_forbidden_files(
        &repo,
        &chain[2],
        "sha2_no_future",
        &["signals.py", "tasks.py", "middleware.py", "decorators.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "models.py",
        "chain2_prior_models.py",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "schemas.py",
        "chain2_prior_schemas.py",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "views.py",
        "chain2_prior_views.py",
        &[
            ("@api_view(['GET'])", true),
            ("def product_list(request):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "serializers.py",
        "chain2_prior_serializers.py",
        &[
            (
                "class ProductSerializer(serializers.ModelSerializer):",
                true,
            ),
            ("model = Product", true),
        ],
    );

    // sha3 = C4': signals.py + tasks.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["signals.py", "tasks.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[3],
        "sha3_no_future",
        &["middleware.py", "decorators.py"],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "models.py",
        "chain3_prior_models.py",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "schemas.py",
        "chain3_prior_schemas.py",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "views.py",
        "chain3_prior_views.py",
        &[
            ("@api_view(['GET'])", true),
            ("def product_list(request):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "serializers.py",
        "chain3_prior_serializers.py",
        &[
            (
                "class ProductSerializer(serializers.ModelSerializer):",
                true,
            ),
            ("model = Product", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "urls.py",
        "chain3_prior_urls.py",
        &[("app_name = 'shop'", true), ("urlpatterns = [", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "permissions.py",
        "chain3_prior_permissions.py",
        &[
            ("class IsOwnerOrReadOnly(BasePermission):", true),
            ("class IsStaff(BasePermission):", true),
        ],
    );

    // sha4 = C5': middleware.py + decorators.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(
        &repo,
        &chain[4],
        "sha4_files",
        &["middleware.py", "decorators.py"],
    );
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "middleware.py",
        "sha4_blame_mw",
        &[
            ("import time", true),
            ("from django.utils.deprecation import MiddlewareMixin", true),
            ("", true),
            ("class RequestTimingMiddleware(MiddlewareMixin):", true),
            ("def process_request(self, request):", true),
            ("request._start_time = time.monotonic()", true),
            ("def process_response(self, request, response):", true),
            ("elapsed = (time.monotonic()", true),
            ("response['X-Response-Time']", true),
            ("return response", true),
        ],
    );
    assert_blame_at_commit(
        &repo,
        &chain[4],
        "decorators.py",
        "sha4_blame_dec",
        &[
            ("from functools import wraps", true),
            ("from django.http import JsonResponse", true),
            ("", true),
            ("def require_json(view_fn):", true),
            ("@wraps(view_fn)", true),
            ("def wrapper(request, *a, **kw):", true),
            ("if request.content_type", true),
            ("return view_fn(request", true),
            ("return wrapper", true),
        ],
    );
    // Verify C1's files (models.py and schemas.py) still correctly attributed at tip.
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "models.py",
        "sha4_models_preserved",
        &[
            ("class Product(models.Model):", true),
            ("name = models.CharField", true),
            ("class Meta: ordering", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "schemas.py",
        "sha4_schemas_preserved",
        &[
            ("class ProductSchema(BaseModel):", true),
            ("price: condecimal", true),
            ("class Config: from_attributes = True", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "views.py",
        "chain4_prior_views.py",
        &[
            ("@api_view(['GET'])", true),
            ("def product_list(request):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "serializers.py",
        "chain4_prior_serializers.py",
        &[
            (
                "class ProductSerializer(serializers.ModelSerializer):",
                true,
            ),
            ("model = Product", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "urls.py",
        "chain4_prior_urls.py",
        &[("app_name = 'shop'", true), ("urlpatterns = [", true)],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "permissions.py",
        "chain4_prior_permissions.py",
        &[
            ("class IsOwnerOrReadOnly(BasePermission):", true),
            ("class IsStaff(BasePermission):", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "signals.py",
        "chain4_prior_signals.py",
        &[
            ("@receiver(post_save, sender=Product)", true),
            ("@receiver(pre_delete, sender=Product)", true),
        ],
    );
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "tasks.py",
        "chain4_prior_tasks.py",
        &[
            ("@shared_task", true),
            ("def sync_inventory(product_id):", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(
    test_fast_path_nested_directory_structure,
    test_fast_path_single_file_grows_across_commits,
    test_fast_path_feature_deletes_file_then_recreates,
    test_fast_path_multi_file_commits_2_files_each,
);
