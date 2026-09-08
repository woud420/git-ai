use super::{
    ExpectedLineExt, TestRepo, assert_note_base_commit_matches, assert_note_files_exact, fs,
    get_commit_chain,
};

// ============================================================================
// END Category 2: Slow Path
// ============================================================================

// ============================================================================
// Category 3: Conflict Resolved by Human
// Feature branch has AI-generated changes that conflict with main branch.
// Human resolves via fs::write (no checkpoint) — conflicted file loses AI
// attribution for that specific rebased commit.  All other AI files in the
// chain retain their attribution.
// ============================================================================

/// Test 1: Python auth.py — feature adds AI login/logout functions, main edits
/// the same file's header comment → conflict on C1.  Human resolves by keeping
/// both parts.  C1' must have NO auth.py in its note; C2'–C5' accumulate other
/// AI files (models.py, views.py, serializers.py, signals.py) normally.
#[test]
fn test_human_conflict_python_auth_c1_conflicts_rest_accumulate() {
    let repo = TestRepo::new();

    // Initial: auth.py with a single line
    repo.commit_untracked_file("auth.py", "# auth module\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: edit auth.py header → will conflict with feature's C1
    repo.commit_untracked_file(
        "auth.py",
        "# authentication module — production\n",
        "main: update auth.py header",
    );
    repo.commit_untracked_file(
        "middleware.py",
        "class AuthMiddleware: pass\n",
        "main: add middleware",
    );
    repo.commit_untracked_file(
        "permissions.py",
        "class IsAuthenticated: pass\n",
        "main: add permissions",
    );
    repo.commit_untracked_file(
        "tokens.py",
        "import secrets\ndef generate_token(): return secrets.token_hex(32)\n",
        "main: add tokens",
    );
    repo.commit_untracked_file(
        "urls.py",
        "from django.urls import path\nurlpatterns = []\n",
        "main: add urls",
    );

    // Feature branch from initial commit (before main's auth.py edit)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI adds login/logout to auth.py — WILL CONFLICT with main's header change
    let mut auth = repo.filename("auth.py");
    auth.set_contents(crate::lines![
        "# auth module",
        "".ai(),
        "def login(username: str, password: str) -> bool:".ai(),
        "    \"\"\"Authenticate user credentials.\"\"\"".ai(),
        "    return username == 'admin' and password == 'secret'".ai(),
        "".ai(),
        "def logout(session_id: str) -> None:".ai(),
        "    \"\"\"Invalidate the given session.\"\"\"".ai(),
        "    pass".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add login and logout to auth.py")
        .unwrap();

    // C2: AI creates models.py
    let mut models = repo.filename("models.py");
    models.set_contents(crate::lines![
        "from dataclasses import dataclass".ai(),
        "".ai(),
        "@dataclass".ai(),
        "class User:".ai(),
        "    id: int".ai(),
        "    username: str".ai(),
        "    email: str".ai(),
        "    is_active: bool = True".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add User model")
        .unwrap();

    // C3: AI creates views.py
    let mut views = repo.filename("views.py");
    views.set_contents(crate::lines![
        "from .auth import login, logout".ai(),
        "".ai(),
        "def login_view(request):".ai(),
        "    ok = login(request.POST['username'], request.POST['password'])".ai(),
        "    return {'ok': ok}".ai(),
        "".ai(),
        "def logout_view(request):".ai(),
        "    logout(request.session['id'])".ai(),
        "    return {'ok': True}".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add login/logout views")
        .unwrap();

    // C4: AI creates serializers.py
    let mut serializers = repo.filename("serializers.py");
    serializers.set_contents(crate::lines![
        "class UserSerializer:".ai(),
        "    fields = ['id', 'username', 'email']".ai(),
        "".ai(),
        "    def serialize(self, user) -> dict:".ai(),
        "        return {f: getattr(user, f) for f in self.fields}".ai(),
        "".ai(),
        "    def deserialize(self, data: dict):".ai(),
        "        return data".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add UserSerializer")
        .unwrap();

    // C5: AI creates signals.py
    let mut signals = repo.filename("signals.py");
    signals.set_contents(crate::lines![
        "from typing import Callable".ai(),
        "".ai(),
        "_handlers: list[Callable] = []".ai(),
        "".ai(),
        "def on_login(fn: Callable) -> Callable:".ai(),
        "    _handlers.append(fn)".ai(),
        "    return fn".ai(),
        "".ai(),
        "def emit_login(user) -> None:".ai(),
        "    for h in _handlers: h(user)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add login signal emitter")
        .unwrap();

    // Rebase onto main — C1 will conflict on auth.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "C1 rebase should conflict on auth.py"
    );

    // Human resolves: keep both header variants merged manually (no checkpoint)
    fs::write(
        repo.path().join("auth.py"),
        "# authentication module — production\n\ndef login(username: str, password: str) -> bool:\n    return username == 'admin' and password == 'secret'\n\ndef logout(session_id: str) -> None:\n    pass\n",
    ).unwrap();
    repo.git(&["add", "auth.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    // Collect rebased chain [C1', C2', C3', C4', C5']
    let chain = get_commit_chain(&repo, 5);

    // C1': human resolved auth.py — AI content survived resolution → auth.py IS in note
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["auth.py"]);

    // C2': models.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["models.py"]);

    // C3': views.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["views.py"]);

    // C4': serializers.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["serializers.py"]);

    // C5': signals.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["signals.py"]);
}

/// Test 4: Python models.py — main adds a class attribute that conflicts with
/// feature's last commit (C5).  All prior AI commits C1'–C4' are attributed
/// normally; C5' loses models.py.
#[test]
fn test_human_conflict_python_models_c5_last_commit_conflicts() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("models.py", "class User:\n    pass\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: adds a class attribute to models.py — conflicts with C5's edit
    repo.commit_untracked_file(
        "models.py",
        "class User:\n    table_name = 'users'\n    pass\n",
        "main: add table_name attribute",
    );
    repo.commit_untracked_file(
        "db.py",
        "import sqlite3\nconn = sqlite3.connect(':memory:')\n",
        "main: add db",
    );
    repo.commit_untracked_file("migrations/__init__.py", "", "main: add migrations package");
    repo.commit_untracked_file(
        "schema.sql",
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n",
        "main: add schema",
    );
    repo.commit_untracked_file("seeds.py", "def seed(): pass\n", "main: add seeds");

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates repository.py
    let mut repo_file = repo.filename("repository.py");
    repo_file.set_contents(crate::lines![
        "from typing import Optional, List".ai(),
        "from .models import User".ai(),
        "".ai(),
        "class UserRepository:".ai(),
        "    def __init__(self): self._store: List[User] = []".ai(),
        "    def save(self, u: User): self._store.append(u)".ai(),
        "    def find(self, id: int) -> Optional[User]: return next((u for u in self._store if u.id == id), None)".ai(),
        "    def all(self) -> List[User]: return list(self._store)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add UserRepository")
        .unwrap();

    // C2: AI creates query_builder.py
    let mut qb = repo.filename("query_builder.py");
    qb.set_contents(crate::lines![
        "class QueryBuilder:".ai(),
        "    def __init__(self, table: str): self.table = table; self._filters: list = []".ai(),
        "    def where(self, **kw): self._filters.append(kw); return self".ai(),
        "    def build(self) -> str:".ai(),
        "        clauses = ' AND '.join(f\"{k}='{v}'\" for d in self._filters for k, v in d.items())".ai(),
        "        return f'SELECT * FROM {self.table}' + (f' WHERE {clauses}' if clauses else '')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add QueryBuilder")
        .unwrap();

    // C3: AI creates validators.py
    let mut val = repo.filename("validators.py");
    val.set_contents(crate::lines![
        "def validate_user(data: dict) -> list[str]:".ai(),
        "    errors = []".ai(),
        "    if not data.get('name'): errors.append('name required')".ai(),
        "    if not data.get('email') or '@' not in data['email']: errors.append('valid email required')".ai(),
        "    if len(data.get('password', '')) < 8: errors.append('password min 8 chars')".ai(),
        "    return errors".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add validate_user")
        .unwrap();

    // C4: AI creates events.py
    let mut events = repo.filename("events.py");
    events.set_contents(crate::lines![
        "from typing import Callable, Dict, List".ai(),
        "_subs: Dict[str, List[Callable]] = {}".ai(),
        "def subscribe(event: str, fn: Callable): _subs.setdefault(event, []).append(fn)".ai(),
        "def publish(event: str, **data): [fn(**data) for fn in _subs.get(event, [])]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add event pub/sub")
        .unwrap();

    // C5: AI edits models.py to add validator — WILL CONFLICT with main's table_name
    let mut models = repo.filename("models.py");
    models.replace_at(
        1,
        "    def validate(self): return bool(getattr(self, 'name', None))".ai(),
    );
    repo.stage_all_and_commit("feat: C5 add validate method to User")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on models.py at C5"
    );

    // Human resolves by keeping all three lines
    fs::write(
        repo.path().join("models.py"),
        "class User:\n    table_name = 'users'\n    def validate(self): return bool(getattr(self, 'name', None))\n    pass\n",
    ).unwrap();
    repo.git(&["add", "models.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': repository.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["repository.py"]);

    // C2': query_builder.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["query_builder.py"]);

    // C3': validators.py only
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["validators.py"]);

    // C4': events.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["events.py"]);

    // C5': models.py human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &[]);
}

/// Test 8: Python pipeline.py — feature starts from a mixed human+AI baseline,
/// then modifies the same file as main in C3.  C1' + C2' accumulate other AI
/// files; C3' loses pipeline.py; C4'–C5' add transform.py and sink.py.
#[test]
fn test_human_conflict_python_pipeline_mixed_baseline_c3_conflict() {
    let repo = TestRepo::new();

    repo.commit_untracked_file("pipeline.py",
        "class Pipeline:\n    def __init__(self): self.stages = []\n    def run(self, data): return data\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: adds a validate method to Pipeline — will conflict with feature C3 adding filter
    repo.commit_untracked_file("pipeline.py",
        "class Pipeline:\n    def __init__(self): self.stages = []\n    def run(self, data): return data\n    def validate(self, data): return bool(data)\n",
        "main: add Pipeline.validate",
    );
    repo.commit_untracked_file("source.py",
        "class FileSource:\n    def __init__(self, path): self.path = path\n    def read(self): return open(self.path).read()\n",
        "main: add FileSource",
    );
    repo.commit_untracked_file("registry.py",
        "_registry = {}\ndef register(name, cls): _registry[name] = cls\ndef get(name): return _registry.get(name)\n",
        "main: add component registry",
    );
    repo.commit_untracked_file("executor.py",
        "from concurrent.futures import ThreadPoolExecutor\nexec_pool = ThreadPoolExecutor(max_workers=4)\n",
        "main: add thread pool executor",
    );
    repo.commit_untracked_file("scheduler.py",
        "import sched, time\ns = sched.scheduler(time.time, time.sleep)\ndef schedule(delay, fn): s.enter(delay, 1, fn)\n",
        "main: add scheduler",
    );

    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: Feature creates source.py (different name on feature branch — no conflict)
    let mut stream = repo.filename("stream.py");
    stream.set_contents(crate::lines![
        "class StreamSource:".ai(),
        "    def __init__(self, gen): self.gen = gen".ai(),
        "    def read(self): return next(self.gen, None)".ai(),
        "    def read_all(self): return list(self.gen)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add StreamSource")
        .unwrap();

    // C2: AI creates filter.py
    let mut filter_file = repo.filename("filter.py");
    filter_file.set_contents(crate::lines![
        "from typing import Callable, TypeVar".ai(),
        "T = TypeVar('T')".ai(),
        "".ai(),
        "class Filter:".ai(),
        "    def __init__(self, pred: Callable): self.pred = pred".ai(),
        "    def apply(self, data: list) -> list: return [x for x in data if self.pred(x)]".ai(),
        "    def negate(self) -> 'Filter': return Filter(lambda x: not self.pred(x))".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add Filter class")
        .unwrap();

    // C3: AI edits pipeline.py to add a filter step — WILL CONFLICT with main's validate
    let mut pipeline = repo.filename("pipeline.py");
    pipeline.replace_at(
        2,
        "    def add_filter(self, f): self.stages.append(f); return self".ai(),
    );
    repo.stage_all_and_commit("feat: C3 add Pipeline.add_filter")
        .unwrap();

    // C4: AI creates transform.py
    let mut transform = repo.filename("transform.py");
    transform.set_contents(crate::lines![
        "class Transform:".ai(),
        "    def __init__(self, fn): self.fn = fn".ai(),
        "    def apply(self, data): return [self.fn(x) for x in data]".ai(),
        "".ai(),
        "class MapTransform(Transform):".ai(),
        "    pass".ai(),
        "".ai(),
        "class FlatMapTransform:".ai(),
        "    def __init__(self, fn): self.fn = fn".ai(),
        "    def apply(self, data): return [y for x in data for y in self.fn(x)]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add Transform classes")
        .unwrap();

    // C5: AI creates sink.py
    let mut sink = repo.filename("sink.py");
    sink.set_contents(crate::lines![
        "from typing import List".ai(),
        "".ai(),
        "class ListSink:".ai(),
        "    def __init__(self): self._items: List = []".ai(),
        "    def write(self, item): self._items.append(item)".ai(),
        "    def flush(self) -> List: r = list(self._items); self._items.clear(); return r".ai(),
        "".ai(),
        "class ConsoleSink:".ai(),
        "    def write(self, item): print(item)".ai(),
        "    def flush(self) -> List: return []".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add Sink classes")
        .unwrap();

    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on pipeline.py at C3"
    );

    // Human resolves by keeping both methods
    fs::write(
        repo.path().join("pipeline.py"),
        "class Pipeline:\n    def __init__(self): self.stages = []\n    def run(self, data): return data\n    def validate(self, data): return bool(data)\n    def add_filter(self, f): self.stages.append(f); return self\n",
    ).unwrap();
    repo.git(&["add", "pipeline.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': stream.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["stream.py"]);

    // C2': filter.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["filter.py"]);

    // C3': pipeline.py human-resolved conflict — AI lines inside diff hunk, attribution dropped
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &[]);

    // C4': transform.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["transform.py"]);

    // C5': sink.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["sink.py"]);
}

crate::reuse_tests_in_worktree!(
    test_human_conflict_python_auth_c1_conflicts_rest_accumulate,
    test_human_conflict_python_models_c5_last_commit_conflicts,
    test_human_conflict_python_pipeline_mixed_baseline_c3_conflict,
);
