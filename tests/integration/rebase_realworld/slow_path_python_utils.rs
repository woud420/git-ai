use super::*;

// ============================================================================
// END Category 1: Fast Path
// ============================================================================

// ============================================================================
// Category 2: Slow Path (same files, no conflict — upstream prepends)
// ============================================================================

/// Test 1: Python utils.py — upstream prepends module header, feature appends
/// validation/sanitization functions. Forces slow path because utils.py blobs
/// differ after rebase (upstream prepended 3 lines, feature appended AI lines).
///
/// Checks that accepted_lines at sha0 is ~8 (not the full-chain ~40), and that
/// no future commit's AI lines appear in earlier notes.
#[test]
fn test_slow_path_python_utils_main_prepends_feature_appends() {
    let repo = TestRepo::new();

    // Initial: utils.py with trailing newline so 3-way merge works cleanly.
    repo.commit_untracked_file("utils.py", "def base_util(): pass\n", "Initial commit");
    let main_branch = repo.current_branch();

    // Main: prepend module header (changes blob → forces slow path on feature commits)
    repo.commit_untracked_file(
        "utils.py",
        "# utils module\nimport logging\n\ndef base_util(): pass\n",
        "main: prepend module header to utils.py",
    );
    // 4 more human commits on different files
    repo.commit_untracked_file(
        "constants.py",
        "MAX_RETRIES = 3\nTIMEOUT = 30\n",
        "main: add constants",
    );
    repo.commit_untracked_file(
        "exceptions.py",
        "class AppError(Exception): pass\nclass ValidationError(AppError): pass\n",
        "main: add exceptions",
    );
    repo.commit_untracked_file(
        "config.py",
        "import os\nDATABASE_URL = os.getenv('DATABASE_URL', 'sqlite:///app.db')\n",
        "main: add config",
    );
    repo.commit_untracked_file(
        "setup.cfg",
        "[metadata]\nname = myapp\nversion = 0.1\n",
        "main: add setup.cfg",
    );

    // Feature branch starts from BEFORE main's prepend (base = initial commit)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append 8 AI lines (validate_email + sanitize_input) to utils.py
    let mut utils = repo.filename("utils.py");
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add validate_email and sanitize_input")
        .unwrap();

    // C2: append 8 more AI lines (normalize + truncate)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add normalize_phone and truncate_text")
        .unwrap();

    // C3: append 8 more AI lines (parse_date + format_currency)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
        "def parse_date(date_str: str, fmt: str = '%Y-%m-%d'):".ai(),
        "    from datetime import datetime".ai(),
        "    return datetime.strptime(date_str, fmt)".ai(),
        "".ai(),
        "def format_currency(amount: float, symbol: str = '$') -> str:".ai(),
        "    return f'{symbol}{amount:,.2f}'".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add parse_date and format_currency")
        .unwrap();

    // C4: append 8 more AI lines (generate_slug + deep_merge)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
        "def parse_date(date_str: str, fmt: str = '%Y-%m-%d'):".ai(),
        "    from datetime import datetime".ai(),
        "    return datetime.strptime(date_str, fmt)".ai(),
        "".ai(),
        "def format_currency(amount: float, symbol: str = '$') -> str:".ai(),
        "    return f'{symbol}{amount:,.2f}'".ai(),
        "".ai(),
        "def generate_slug(text: str) -> str:".ai(),
        "    import re".ai(),
        "    return re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')".ai(),
        "".ai(),
        "def deep_merge(base: dict, override: dict) -> dict:".ai(),
        "    result = dict(base)".ai(),
        "    for k, v in override.items():".ai(),
        "        result[k] = deep_merge(base[k], v) if isinstance(v, dict) and isinstance(base.get(k), dict) else v".ai(),
        "    return result".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add generate_slug and deep_merge")
        .unwrap();

    // C5: append 8 more AI lines (retry_with_backoff + chunk_list)
    utils.set_contents(crate::lines![
        "def base_util(): pass",
        "".ai(),
        "def validate_email(email: str) -> bool:".ai(),
        "    import re".ai(),
        "    pattern = r'^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\\.[a-zA-Z]{2,}$'".ai(),
        "    return bool(re.match(pattern, email))".ai(),
        "".ai(),
        "def sanitize_input(text: str) -> str:".ai(),
        "    return text.strip().replace('<', '&lt;').replace('>', '&gt;')".ai(),
        "".ai(),
        "def normalize_phone(phone: str) -> str:".ai(),
        "    import re".ai(),
        "    digits = re.sub(r'\\D', '', phone)".ai(),
        "    return f'+1{digits}' if len(digits) == 10 else digits".ai(),
        "".ai(),
        "def truncate_text(text: str, max_len: int, suffix: str = '...') -> str:".ai(),
        "    return text if len(text) <= max_len else text[:max_len - len(suffix)] + suffix".ai(),
        "".ai(),
        "def parse_date(date_str: str, fmt: str = '%Y-%m-%d'):".ai(),
        "    from datetime import datetime".ai(),
        "    return datetime.strptime(date_str, fmt)".ai(),
        "".ai(),
        "def format_currency(amount: float, symbol: str = '$') -> str:".ai(),
        "    return f'{symbol}{amount:,.2f}'".ai(),
        "".ai(),
        "def generate_slug(text: str) -> str:".ai(),
        "    import re".ai(),
        "    return re.sub(r'[^a-z0-9]+', '-', text.lower()).strip('-')".ai(),
        "".ai(),
        "def deep_merge(base: dict, override: dict) -> dict:".ai(),
        "    result = dict(base)".ai(),
        "    for k, v in override.items():".ai(),
        "        result[k] = deep_merge(base[k], v) if isinstance(v, dict) and isinstance(base.get(k), dict) else v".ai(),
        "    return result".ai(),
        "".ai(),
        "def retry_with_backoff(fn, attempts: int = 3, base_delay: float = 0.5):".ai(),
        "    import time".ai(),
        "    for i in range(attempts):".ai(),
        "        try: return fn()".ai(),
        "        except Exception:".ai(),
        "            if i == attempts - 1: raise".ai(),
        "            time.sleep(base_delay * (2 ** i))".ai(),
        "".ai(),
        "def chunk_list(lst: list, size: int) -> list:".ai(),
        "    return [lst[i:i+size] for i in range(0, len(lst), size)]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add retry_with_backoff and chunk_list")
        .unwrap();

    // Rebase feature onto main (non-conflicting: prepend + append)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': note has utils.py only
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["utils.py"]);

    // sha0 blame: first 3 lines human (# utils module, import logging, blank),
    // then def base_util (human), then 8 AI lines
    assert_blame_at_commit(
        &repo,
        &chain[0],
        "utils.py",
        "sha0_blame",
        &[
            ("# utils module", false),
            ("import logging", false),
            ("", false),
            ("def base_util(): pass", false),
            ("", true),
            ("def validate_email(email: str) -> bool:", true),
            ("import re", true),
            ("pattern = r'^", true),
            ("return bool(re.match(pattern, email))", true),
            ("", true),
            ("def sanitize_input(text: str) -> str:", true),
            ("return text.strip()", true),
        ],
    );

    // sha1 = C2'
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "utils.py",
        "sha1_blame_new",
        &[("def normalize_phone", true), ("def truncate_text", true)],
    );

    // sha2 = C3'
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "utils.py",
        "sha2_blame_new",
        &[("def parse_date", true), ("def format_currency", true)],
    );

    // sha3 = C4'
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "utils.py",
        "sha3_blame_new",
        &[("def generate_slug", true), ("def deep_merge", true)],
    );

    // sha4 = C5'
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["utils.py"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "utils.py",
        "sha4_blame_new",
        &[("def retry_with_backoff", true), ("def chunk_list", true)],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_python_utils_main_prepends_feature_appends,);
