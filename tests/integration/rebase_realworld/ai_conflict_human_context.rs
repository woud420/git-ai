use super::*;

/// Test 3: processor.py — feature (C3) adds 5 AI lines to method2 body,
/// main also changes method2.  AI resolution rewrites processor.py preserving
/// 2 human context lines and writing 7 lines for the resolved method2 (marked
/// `.ai()` in set_contents).  However, the content-diff path only carries
/// attribution for lines whose content exactly matches the original feature commit:
/// only `def method2(self):`, `result = []`, `for i in range(10):`, and
/// `result.append(i * 2)` survive the content match — 4 lines.  The newly
/// introduced lines (`# AI merged`, `label = `, `return result, label`) have no
/// entry in `original_head_line_to_author` and therefore receive human attribution.
#[test]
fn test_conflict_ai_resolves_preserving_human_context_lines() {
    run_test_conflict_ai_resolves_preserving_human_context_lines(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_preserving_human_context_lines_standard_human() {
    run_test_conflict_ai_resolves_preserving_human_context_lines(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_preserving_human_context_lines(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: processor.py with a class (6 human lines)
    repo.commit_untracked_file("processor.py",
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self): pass\n    def method3(self): return 'method3'\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: human changes method2 differently → conflict
    repo.commit_untracked_file("processor.py",
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self): return 'human-method2'\n    def method3(self): return 'method3'\n",
        "main: implement method2",
    );
    repo.commit_untracked_file(
        "runner.py",
        "from processor import Processor\np = Processor()\np.method1()\n",
        "main: add runner",
    );
    repo.commit_untracked_file("tests/test_processor.py",
        "from processor import Processor\ndef test_method1(): assert Processor().method1() == 'method1'\n",
        "main: add tests",
    );
    repo.commit_untracked_file(
        "setup.py",
        "from setuptools import setup\nsetup(name='processor', version='0.1.0')\n",
        "main: add setup.py",
    );
    repo.commit_untracked_file(
        "pyproject.toml",
        "[build-system]\nrequires = ['setuptools']\n",
        "main: add pyproject.toml",
    );

    // Feature branch from base
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates util_a.py (8 AI lines)
    let mut util_a = repo.filename("util_a.py");
    util_a.set_contents(crate::lines![
        "def parse_int(s: str) -> int:".ai(),
        "    try:".ai(),
        "        return int(s)".ai(),
        "    except ValueError:".ai(),
        "        raise ValueError(f'Cannot parse {s!r} as int')".ai(),
        "".ai(),
        "def parse_float(s: str) -> float:".ai(),
        "    return float(s)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add util_a").unwrap();

    // C2: AI creates util_b.py (8 AI lines)
    let mut util_b = repo.filename("util_b.py");
    util_b.set_contents(crate::lines![
        "from typing import List, Optional".ai(),
        "".ai(),
        "def chunk(lst: List, size: int) -> List[List]:".ai(),
        "    return [lst[i:i+size] for i in range(0, len(lst), size)]".ai(),
        "".ai(),
        "def flatten(lst: List[List]) -> List:".ai(),
        "    return [x for sub in lst for x in sub]".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add util_b").unwrap();

    // C3: AI adds 5 lines to method2 in processor.py — WILL CONFLICT
    let mut processor = repo.filename("processor.py");
    processor.set_contents(crate::lines![
        human_context.expected_line("class Processor:"),
        human_context.expected_line("    def method1(self): return 'method1'"),
        "    def method2(self):".ai(),
        "        result = []".ai(),
        "        for i in range(10):".ai(),
        "            result.append(i * 2)".ai(),
        "        return result".ai(),
        human_context.expected_line("    def method3(self): return 'method3'"),
    ]);
    fs::write(
        repo.path().join("processor.py"),
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self):\n        result = []\n        for i in range(10):\n            result.append(i * 2)\n        return result\n    def method3(self): return 'method3'\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "processor.py"])
        .unwrap();
    repo.stage_all_and_commit("feat: C3 AI implements method2")
        .unwrap();

    // C4: AI creates util_d.py (8 AI lines)
    let mut util_d = repo.filename("util_d.py");
    util_d.set_contents(crate::lines![
        "import hashlib".ai(),
        "".ai(),
        "def md5(s: str) -> str:".ai(),
        "    return hashlib.md5(s.encode()).hexdigest()".ai(),
        "".ai(),
        "def sha256(s: str) -> str:".ai(),
        "    return hashlib.sha256(s.encode()).hexdigest()".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add util_d").unwrap();

    // C5: AI creates util_e.py (8 AI lines)
    let mut util_e = repo.filename("util_e.py");
    util_e.set_contents(crate::lines![
        "import json".ai(),
        "".ai(),
        "def to_json(obj) -> str:".ai(),
        "    return json.dumps(obj, indent=2)".ai(),
        "".ai(),
        "def from_json(s: str):".ai(),
        "    return json.loads(s)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add util_e").unwrap();

    // Rebase — C3 will conflict on processor.py
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(
        rebase_result.is_err(),
        "rebase should conflict on processor.py at C3"
    );

    // AI resolves: 2 human context lines + 7 lines for resolved method2 (set_contents(.ai()))
    // NOTE: content-diff only recovers lines matching original C3 content:
    //   def method2, result = [], for i in range, result.append → 4 AI-attributed lines.
    //   # AI merged, label = , return result/label → newly introduced, no original match → human.
    let mut conflict_processor = repo.filename("processor.py");
    conflict_processor.set_contents(crate::lines![
        human_context.expected_line("class Processor:"),
        human_context.expected_line("    def method1(self): return 'method1'"),
        "    def method2(self):".ai(),
        "        # AI merged: combines human's return with feature's loop".ai(),
        "        result = []".ai(),
        "        for i in range(10):".ai(),
        "            result.append(i * 2)".ai(),
        "        label = 'human-method2'".ai(),
        "        return result, label".ai(),
        human_context.expected_line("    def method3(self): return 'method3'"),
    ]);
    fs::write(
        repo.path().join("processor.py"),
        "class Processor:\n    def method1(self): return 'method1'\n    def method2(self):\n        # AI merged: combines human's return with feature's loop\n        result = []\n        for i in range(10):\n            result.append(i * 2)\n        label = 'human-method2'\n        return result, label\n    def method3(self): return 'method3'\n",
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "processor.py"])
        .unwrap();
    repo.git(&["add", "processor.py"]).unwrap();
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': util_a.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["util_a.py"]);

    // C2': util_b.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["util_b.py"]);

    // C3': processor.py only (AI-resolved: 4 AI lines via content-diff match)
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["processor.py"]);

    // blame at chain[2] for processor.py: lines from parent are human,
    // all new lines written by AI during resolution are AI.
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "processor.py",
        "c3_blame_processor",
        &[
            ("class Processor:", false),
            ("def method1", false),
            ("def method2", true),
            ("AI merged", true),
            ("result = []", true),
            ("for i in range", true),
            ("result.append", true),
            ("label = ", true),
            ("return result, label", true),
            ("def method3", false),
        ],
    );

    // C4': util_d.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["util_d.py"]);

    // C5': util_e.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["util_e.py"]);

    human_context.assert_metadata_humans(&repo, &chain[2], "c3'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_preserving_human_context_lines,
    test_conflict_ai_resolves_preserving_human_context_lines_standard_human,
);
