use super::*;

const PRIOR_SAMPLES: &[PriorBlameSample] = &[
    (
        "service.py",
        "c1_service.py",
        ["class DataService:", "def fetch(self, key):"],
    ),
    (
        "service.py",
        "c2_service.py",
        ["def delete(self, key):", "def exists(self, key):"],
    ),
];

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
    assert_prior_blame_samples(&repo, &chain[2], 2, &PRIOR_SAMPLES[0..1]);

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
    assert_prior_blame_samples(&repo, &chain[3], 3, &PRIOR_SAMPLES[0..2]);

    // sha4 = C5': service.py (C5's delta only; fast-path remaps original note)
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["service.py"]);
}

crate::reuse_tests_in_worktree!(test_fast_path_single_file_grows_across_commits,);
