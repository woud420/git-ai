use super::*;

/// Test 4: TOML config file — upstream prepends production header, feature
/// appends new TOML sections per commit. Each commit adds 8+ AI lines.
/// Verifies no future sections bleed into earlier commit notes.
#[test]
fn test_slow_path_config_file_both_add_different_sections() {
    let repo = TestRepo::new();

    // Initial: config.toml with trailing newline
    repo.commit_untracked_file(
        "config.toml",
        "[server]\nhost = \"localhost\"\nport = 8080\n",
        "Initial commit",
    );
    let main_branch = repo.current_branch();

    // Main: prepend production comment (forces slow path on feature commits)
    repo.commit_untracked_file(
        "config.toml",
        "# Production config\n\n[server]\nhost = \"localhost\"\nport = 8080\n",
        "main: prepend production config header",
    );
    repo.commit_untracked_file(
        ".env.production",
        "APP_ENV=production\nLOG_LEVEL=warn\n",
        "main: add production env",
    );
    repo.commit_untracked_file(
        "docker-compose.prod.yml",
        "version: '3.9'\nservices:\n  app:\n    image: myapp:latest\n    ports: ['80:8080']\n",
        "main: add prod docker-compose",
    );
    repo.commit_untracked_file(
        "nginx.conf",
        "server { listen 80; location / { proxy_pass http://app:8080; } }\n",
        "main: add nginx config",
    );
    repo.commit_untracked_file(
        "Makefile",
        "deploy:\n\tdocker-compose -f docker-compose.prod.yml up -d\n",
        "main: add Makefile",
    );

    // Feature branch from before main's prepend
    let base_sha = repo
        .git(&["rev-parse", "HEAD~5"])
        .unwrap()
        .trim()
        .to_string();
    repo.git(&["checkout", "-b", "feature", &base_sha]).unwrap();

    // C1: append [database] section (8 AI lines)
    let mut cfg = repo.filename("config.toml");
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "min_connections = 5".ai(),
        "connect_timeout = 30".ai(),
        "idle_timeout = 600".ai(),
        "max_lifetime = 1800".ai(),
        "ssl_mode = \"require\"".ai(),
    ]);
    repo.stage_all_and_commit("feat: C1 add [database] config section")
        .unwrap();

    // C2: append [cache] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "min_connections = 5".ai(),
        "connect_timeout = 30".ai(),
        "idle_timeout = 600".ai(),
        "max_lifetime = 1800".ai(),
        "ssl_mode = \"require\"".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "url = \"redis://localhost:6379/0\"".ai(),
        "max_size = 1000".ai(),
        "ttl_seconds = 300".ai(),
        "eviction_policy = \"lru\"".ai(),
        "compression = true".ai(),
        "key_prefix = \"app:\"".ai(),
        "serializer = \"json\"".ai(),
    ]);
    repo.stage_all_and_commit("feat: C2 add [cache] config section")
        .unwrap();

    // C3: append [metrics] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "ssl_mode = \"require\"".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "url = \"redis://localhost:6379/0\"".ai(),
        "ttl_seconds = 300".ai(),
        "".ai(),
        "[metrics]".ai(),
        "enabled = true".ai(),
        "endpoint = \"/metrics\"".ai(),
        "port = 9090".ai(),
        "interval_seconds = 15".ai(),
        "include_system = true".ai(),
        "labels = [\"app\", \"env\", \"version\"]".ai(),
        "exporter = \"prometheus\"".ai(),
        "histogram_buckets = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]".ai(),
    ]);
    repo.stage_all_and_commit("feat: C3 add [metrics] config section")
        .unwrap();

    // C4: append [auth] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "max_connections = 100".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "url = \"redis://localhost:6379/0\"".ai(),
        "".ai(),
        "[metrics]".ai(),
        "enabled = true".ai(),
        "endpoint = \"/metrics\"".ai(),
        "".ai(),
        "[auth]".ai(),
        "provider = \"jwt\"".ai(),
        "secret_env = \"JWT_SECRET\"".ai(),
        "token_expiry_seconds = 3600".ai(),
        "refresh_expiry_seconds = 86400".ai(),
        "algorithm = \"HS256\"".ai(),
        "issuer = \"myapp\"".ai(),
        "audience = [\"web\", \"mobile\"]".ai(),
        "allow_anonymous = false".ai(),
    ]);
    repo.stage_all_and_commit("feat: C4 add [auth] config section")
        .unwrap();

    // C5: append [notifications] section (8 AI lines)
    cfg.set_contents(crate::lines![
        "[server]",
        "host = \"localhost\"",
        "port = 8080",
        "".ai(),
        "[database]".ai(),
        "url = \"postgres://user:pass@localhost:5432/mydb\"".ai(),
        "".ai(),
        "[cache]".ai(),
        "backend = \"redis\"".ai(),
        "".ai(),
        "[metrics]".ai(),
        "enabled = true".ai(),
        "".ai(),
        "[auth]".ai(),
        "provider = \"jwt\"".ai(),
        "token_expiry_seconds = 3600".ai(),
        "".ai(),
        "[notifications]".ai(),
        "email_driver = \"smtp\"".ai(),
        "smtp_host = \"smtp.sendgrid.net\"".ai(),
        "smtp_port = 587".ai(),
        "smtp_user_env = \"SMTP_USER\"".ai(),
        "smtp_pass_env = \"SMTP_PASS\"".ai(),
        "from_address = \"noreply@myapp.com\"".ai(),
        "queue_name = \"notifications\"".ai(),
        "retry_attempts = 3".ai(),
    ]);
    repo.stage_all_and_commit("feat: C5 add [notifications] config section")
        .unwrap();

    // Rebase onto main (non-conflicting)
    repo.git(&["rebase", &main_branch]).unwrap();

    let chain = get_commit_chain(&repo, 5);

    // sha0 = C1': config.toml with [database] section only
    assert_note_base_commit_matches(&repo, &chain[0], "sha0");
    assert_note_files_exact(&repo, &chain[0], "sha0_files", &["config.toml"]);

    // sha1 = C2': [cache] section only
    assert_note_base_commit_matches(&repo, &chain[1], "sha1");
    assert_note_files_exact(&repo, &chain[1], "sha1_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[1],
        "config.toml",
        "sha1_blame_new",
        &[
            ("[cache]", true),
            ("backend = \"redis\"", true),
            ("eviction_policy", true),
        ],
    );

    // sha2 = C3': [metrics] section only
    assert_note_base_commit_matches(&repo, &chain[2], "sha2");
    assert_note_files_exact(&repo, &chain[2], "sha2_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[2],
        "config.toml",
        "sha2_blame_new",
        &[
            ("[metrics]", true),
            ("exporter = \"prometheus\"", true),
            ("histogram_buckets", true),
        ],
    );

    // sha3 = C4': [auth] section only
    assert_note_base_commit_matches(&repo, &chain[3], "sha3");
    assert_note_files_exact(&repo, &chain[3], "sha3_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[3],
        "config.toml",
        "sha3_blame_new",
        &[
            ("[auth]", true),
            ("provider = \"jwt\"", true),
            ("allow_anonymous = false", true),
        ],
    );

    // sha4 = C5': [notifications] section only
    assert_note_base_commit_matches(&repo, &chain[4], "sha4");
    assert_note_files_exact(&repo, &chain[4], "sha4_files", &["config.toml"]);
    assert_blame_sample_at_commit(
        &repo,
        &chain[4],
        "config.toml",
        "sha4_blame_new",
        &[
            ("[notifications]", true),
            ("email_driver = \"smtp\"", true),
            ("retry_attempts = 3", true),
        ],
    );
}

crate::reuse_tests_in_worktree!(test_slow_path_config_file_both_add_different_sections,);
