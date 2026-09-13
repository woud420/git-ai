use super::*;

/// Test 6: config.py AND settings.py both conflict in C3.
/// C3 AI changes a line in both files; main also changes same lines.
/// AI resolves both conflicts.  Note for C3' has both files.
#[test]
fn test_conflict_ai_resolves_multiple_files_in_same_commit() {
    run_test_conflict_ai_resolves_multiple_files_in_same_commit(HumanContextAttribution::Known);
}

#[test]
fn test_conflict_ai_resolves_multiple_files_in_same_commit_standard_human() {
    run_test_conflict_ai_resolves_multiple_files_in_same_commit(
        HumanContextAttribution::Unattributed,
    );
}

fn run_test_conflict_ai_resolves_multiple_files_in_same_commit(
    human_context: HumanContextAttribution,
) {
    let repo = TestRepo::new();

    // Initial: BOTH files exist at the shared base so C3's edits will conflict with main
    repo.commit_untracked_file(
        "config.py",
        "DEBUG = False\nSECRET_KEY = 'changeme'\n",
        "Initial: config",
    );
    repo.commit_untracked_file(
        "settings.py",
        "DATABASE_URL = 'sqlite:///dev.db'\nCACHE_BACKEND = 'locmem'\n",
        "Initial: settings",
    );
    let main_branch = repo.current_branch();

    // Main: changes the same lines in both files → will conflict with feature's C3
    repo.commit_untracked_file(
        "config.py",
        "DEBUG = True\nSECRET_KEY = 'changeme'\n",
        "main: enable DEBUG",
    );
    repo.commit_untracked_file(
        "settings.py",
        "DATABASE_URL = 'postgres://localhost/main_db'\nCACHE_BACKEND = 'redis'\n",
        "main: update settings",
    );
    repo.commit_untracked_file(
        "wsgi.py",
        "from app import create_app\napplication = create_app()\n",
        "main: add wsgi",
    );
    repo.commit_untracked_file(
        "asgi.py",
        "from app import create_app\napplication = create_app()\n",
        "main: add asgi",
    );
    repo.commit_untracked_file(
        "manage.py",
        "#!/usr/bin/env python\nimport sys\nif __name__ == '__main__': pass\n",
        "main: add manage.py",
    );

    // Feature branch from the shared base (HEAD~5 = after both initial commits)
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: AI creates auth.py (8 AI lines)
    let mut auth = repo.filename("auth.py");
    auth.set_contents(crate::lines![
        "from typing import Optional".ai(),
        "".ai(),
        "def authenticate(token: str) -> Optional[str]:".ai(),
        "    if not token: return None".ai(),
        "    parts = token.split('.')".ai(),
        "    if len(parts) != 3: return None".ai(),
        "    return parts[1]".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add auth").unwrap();

    // C2: AI creates middleware.py (8 AI lines)
    let mut middleware = repo.filename("middleware.py");
    middleware.set_contents(crate::lines![
        "class CorsMiddleware:".ai(),
        "    def __init__(self, app):".ai(),
        "        self.app = app".ai(),
        "    def __call__(self, environ, start_response):".ai(),
        "        def custom_start(status, headers):".ai(),
        "            headers.append(('Access-Control-Allow-Origin', '*'))".ai(),
        "            return start_response(status, headers)".ai(),
        "        return self.app(environ, custom_start)".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add CORS middleware")
        .unwrap();

    // C3: AI changes config.py AND settings.py — BOTH WILL CONFLICT
    let mut config = repo.filename("config.py");
    config.set_contents(crate::lines![
        human_context.expected_line("DEBUG = False"),
        "SECRET_KEY = 'ai-generated-secret-key-v2'".ai(),
    ]);
    let mut settings = repo.filename("settings.py");
    settings.set_contents(crate::lines![
        "DATABASE_URL = 'postgres://localhost/feature_db'".ai(),
        human_context.expected_line("CACHE_BACKEND = 'locmem'"),
    ]);
    repo.stage_all_and_commit("feat: C3 AI tunes config and settings")
        .unwrap();

    // C4: AI creates permissions.py (8 AI lines)
    let mut permissions = repo.filename("permissions.py");
    permissions.set_contents(crate::lines![
        "class Permission:".ai(),
        "    READ = 'read'".ai(),
        "    WRITE = 'write'".ai(),
        "    ADMIN = 'admin'".ai(),
        "".ai(),
        "def has_permission(user_perms: list, required: str) -> bool:".ai(),
        "    return required in user_perms".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add permissions")
        .unwrap();

    // C5: AI creates serializers.py (8 AI lines)
    let mut serializers = repo.filename("serializers.py");
    serializers.set_contents(crate::lines![
        "import json".ai(),
        "".ai(),
        "class JsonSerializer:".ai(),
        "    @staticmethod".ai(),
        "    def dumps(obj) -> str: return json.dumps(obj)".ai(),
        "    @staticmethod".ai(),
        "    def loads(s: str): return json.loads(s)".ai(),
        "".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add JSON serializer")
        .unwrap();

    // Rebase — C3 will conflict on config.py (and possibly settings.py)
    repo.git(&["checkout", "feature"]).unwrap();
    let rebase_result = repo.git(&["rebase", &main_branch]);
    assert!(rebase_result.is_err(), "rebase should conflict at C3");

    // AI resolves config.py
    let mut conflict_config = repo.filename("config.py");
    conflict_config.set_contents(crate::lines![
        human_context.expected_line("DEBUG = True"),
        "SECRET_KEY = 'ai-generated-secret-key-v2'".ai(),
    ]);
    // AI resolves settings.py
    let mut conflict_settings = repo.filename("settings.py");
    conflict_settings.set_contents(crate::lines![
        "DATABASE_URL = 'postgres://localhost/feature_db'".ai(),
        human_context.expected_line("CACHE_BACKEND = 'redis'"),
    ]);
    repo.git_with_env(&["rebase", "--continue"], &[("GIT_EDITOR", "true")], None)
        .unwrap();

    let chain = get_commit_chain(&repo, 5);

    // C1': auth.py only (per-commit-delta)
    assert_note_base_commit_matches(&repo, &chain[0], "c1_base");
    assert_note_files_exact(&repo, &chain[0], "c1_files", &["auth.py"]);

    // C2': middleware.py only
    assert_note_base_commit_matches(&repo, &chain[1], "c2_base");
    assert_note_files_exact(&repo, &chain[1], "c2_files", &["middleware.py"]);

    // C3': config.py + settings.py (AI-resolved, both in same commit)
    assert_note_base_commit_matches(&repo, &chain[2], "c3_base");
    assert_note_files_exact(&repo, &chain[2], "c3_files", &["config.py", "settings.py"]);

    // blame for config.py: DEBUG is human (unchanged), SECRET_KEY is AI
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "config.py",
        "c3_blame_config",
        &[
            ("DEBUG = True", false),
            ("SECRET_KEY = 'ai-generated-secret-key-v2'", true),
        ],
    );

    // blame for settings.py: DATABASE_URL is AI, CACHE_BACKEND is human
    assert_blame_at_commit(
        &repo,
        &chain[2],
        "settings.py",
        "c3_blame_settings",
        &[
            ("DATABASE_URL = 'postgres://localhost/feature_db'", true),
            ("CACHE_BACKEND = 'redis'", false),
        ],
    );

    // C4': permissions.py only
    assert_note_base_commit_matches(&repo, &chain[3], "c4_base");
    assert_note_files_exact(&repo, &chain[3], "c4_files", &["permissions.py"]);

    // C5': serializers.py only
    assert_note_base_commit_matches(&repo, &chain[4], "c5_base");
    assert_note_files_exact(&repo, &chain[4], "c5_files", &["serializers.py"]);

    human_context.assert_metadata_humans(&repo, &chain[2], "c3'");
}

crate::reuse_tests_in_worktree!(
    test_conflict_ai_resolves_multiple_files_in_same_commit,
    test_conflict_ai_resolves_multiple_files_in_same_commit_standard_human,
);
