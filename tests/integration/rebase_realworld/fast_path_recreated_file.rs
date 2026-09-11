use super::*;

const PRIOR_SAMPLES: &[PriorBlameSample] = &[
    (
        "temp_module.py",
        "temp_module.py",
        ["class TempProcessor:", "def process(self, data):"],
    ),
    (
        "util_a.py",
        "util_a.py",
        ["def parse_csv(path):", "def write_csv(path, rows, fields):"],
    ),
    (
        "util_b.py",
        "util_b.py",
        ["def load_json(path):", "def save_json(path, data"],
    ),
    (
        "util_c.py",
        "util_c.py",
        ["def md5(data):", "def sha256(data):"],
    ),
    (
        "util_d.py",
        "util_d.py",
        ["def retry(fn: Callable", "def memoize(fn: Callable"],
    ),
];

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
    assert_prior_blame_samples(&repo, &chain[1], 1, &PRIOR_SAMPLES[0..2]);

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
    assert_prior_blame_samples(&repo, &chain[2], 2, &PRIOR_SAMPLES[1..3]);

    // sha3 = C3_util_c': util_c.py
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["util_c.py"]);
    assert_note_no_forbidden_files(
        &repo,
        &chain[3],
        "sha3_no_temp_or_future",
        &["temp_module.py", "util_d.py", "util_e.py"],
    );
    assert_prior_blame_samples(&repo, &chain[3], 3, &PRIOR_SAMPLES[1..3]);

    // sha4 = C4': util_d.py
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["util_d.py"]);
    assert_note_no_forbidden_files(&repo, &chain[4], "sha4_no_future", &["util_e.py"]);
    assert_prior_blame_samples(&repo, &chain[4], 4, &PRIOR_SAMPLES[1..4]);

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
    assert_prior_blame_samples(&repo, &chain[5], 5, &PRIOR_SAMPLES[1..2]);
    assert_prior_blame_samples(&repo, &chain[5], 5, &PRIOR_SAMPLES[3..5]);

    // Note: accepted_lines is NOT monotonic here because chain[2] is a human deletion commit
    // (removes temp_module.py) which has 0 accepted lines, breaking the monotonic property.
}

crate::reuse_tests_in_worktree!(test_fast_path_feature_deletes_file_then_recreates,);
