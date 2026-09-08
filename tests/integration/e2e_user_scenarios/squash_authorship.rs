#[cfg(not(target_os = "windows"))]
use super::write_executable_script;
use super::{TestRepo, assert_stats, assert_tool_model, commit_stats, fs, head_stats};

// ---------------------------------------------------------------------------
// Test 12: AI refactors its own code (SKIPPED — issue #162)
// ---------------------------------------------------------------------------
#[test]
#[ignore = "https://github.com/git-ai-project/git-ai/issues/162"]
fn test_squash_authorship_ai_refactor() {
    let _repo = TestRepo::new();
}

// ---------------------------------------------------------------------------
// Test 17: interactive rebase with squash preserves authorship
// ---------------------------------------------------------------------------
#[test]
#[cfg(not(target_os = "windows"))]
fn test_interactive_rebase_squash_preserves_authorship() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("api_handler.py");

    // Base commit
    let base_content = "\
from flask import Flask, request, jsonify

app = Flask(__name__)

# API endpoint placeholder
";
    fs::write(&file_path, base_content).unwrap();
    repo.git(&["add", "api_handler.py"]).unwrap();
    repo.commit("Base commit with initial API structure")
        .unwrap();

    let base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // COMMIT 1: Human adds 2 lines, AI adds 3 lines
    let human_edit1 = "\
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/api/users', methods=['POST'])
def create_user():
# API endpoint placeholder
";
    fs::write(&file_path, human_edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    let ai_edit1 = "\
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/api/users', methods=['POST'])
def create_user():
    data = request.get_json()
    username = data.get('username', '') if data else ''
    return jsonify({'user': username}), 201
# API endpoint placeholder
";
    fs::write(&file_path, ai_edit1).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "api_handler.py"])
        .unwrap();

    repo.git(&["add", "api_handler.py"]).unwrap();
    repo.commit("Commit 1: Add user creation endpoint with basic implementation")
        .unwrap();

    let stats1 = head_stats(&repo);
    assert_stats(&stats1, 2, 3, 3, 0, 5);

    // COMMIT 2: Human adds 2 lines, AI deletes 1 AI line and adds 2 lines
    let human_edit2 = "\
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/api/users', methods=['POST'])
def create_user():
    data = request.get_json()
    username = data.get('username', '') if data else ''
    return jsonify({'user': username}), 201
    # TODO: Add proper database integration
    # TODO: Add authentication check
# API endpoint placeholder
";
    fs::write(&file_path, human_edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_known_human"]).unwrap();

    let ai_edit2 = "\
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/api/users', methods=['POST'])
def create_user():
    data = request.get_json()
    username = data.get('username', '') if data else ''
    # TODO: Add proper database integration
    # TODO: Add authentication check
    if not username or len(username) < 3:
        return jsonify({'error': 'Invalid username'}), 400
# API endpoint placeholder
";
    fs::write(&file_path, ai_edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "api_handler.py"])
        .unwrap();

    repo.git(&["add", "api_handler.py"]).unwrap();
    repo.commit("Commit 2: Add documentation and improve validation")
        .unwrap();

    let stats2 = head_stats(&repo);
    assert_stats(&stats2, 2, 2, 2, 1, 4);

    // Interactive rebase: squash last 2 commits into 1
    let script_content = "#!/bin/sh\n\
        sed -i.bak '2s/pick/squash/' \"$1\"\n";
    let script_path = repo.path().join("squash_script.sh");
    write_executable_script(&script_path, script_content).unwrap();

    repo.git_with_env(
        &["rebase", "-i", &base_sha],
        &[
            ("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap()),
            ("GIT_EDITOR", "true"),
        ],
        None,
    )
    .expect("Interactive rebase with squash should succeed");

    let squashed_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Verify one commit after base
    let count = repo
        .git(&["rev-list", "--count", &format!("{base_sha}..HEAD")])
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(count, "1", "Should have exactly 1 commit after squash");

    let blame_after = repo.git_ai(&["blame", "api_handler.py"]).unwrap();
    assert!(blame_after.contains("mock_ai"));
    assert!(blame_after.contains("Test User"));

    let squashed_stats = commit_stats(&repo, &["stats", &squashed_sha, "--json"]);
    assert_stats(&squashed_stats, 4, 4, 4, 0, 8);
    assert_tool_model(&squashed_stats, "mock_ai::unknown", 4, 4);
}

// ---------------------------------------------------------------------------
// squash merge concatenates AI and human changes
//
// Originally `test_squash_authorship_concatenates`, which drove the removed
// `git-ai squash-authorship` command directly. That command was deleted in the
// rewrite; squash attribution now flows through the unified
// `git-ai ci local merge` path. The scenario (a 5-line file edited across two
// human+AI commits, then squashed) is preserved; assertions reflect the new
// content-based attribution logic.
// ---------------------------------------------------------------------------
#[test]
fn test_squash_authorship_concatenates() {
    let repo = TestRepo::new();
    let file_path = repo.path().join("example.txt");

    // Anchor commit so we have a valid HEAD / base.
    fs::write(repo.path().join("README.md"), "# Test\n").unwrap();
    repo.git(&["add", "README.md"]).unwrap();
    repo.commit("Initial commit").unwrap();
    let base_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();

    // Initial file with 5 lines.
    let initial = "\
Line 1: Initial
Line 2: Initial
Line 3: Initial
Line 4: Initial
Line 5: Initial
";
    fs::write(&file_path, initial).unwrap();
    repo.git(&["add", "example.txt"]).unwrap();
    repo.commit("Initial file with 5 lines").unwrap();

    // Feature branch carrying the human+AI commit stack.
    repo.git(&["checkout", "-b", "feature"]).unwrap();

    // COMMIT 1: human adds 2 lines, AI adds 3 and deletes 2.
    let human_edit = "\
Line 1: Initial
Line 2: Initial
H: Human Line 1
H: Human Line 2
Line 3: Initial
Line 4: Initial
Line 5: Initial
";
    repo.human_edit("example.txt", human_edit);

    let ai_edit = "\
Line 1: Initial
H: Human Line 1
H: Human Line 2
AI: AI Line 1
AI: AI Line 2
AI: AI Line 3
Line 4: Initial
Line 5: Initial
";
    fs::write(&file_path, ai_edit).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    repo.git(&["add", "example.txt"]).unwrap();
    repo.commit("Commit 1: Human adds 2, AI adds 3 and deletes 2")
        .unwrap();
    let stats1 = head_stats(&repo);
    assert_stats(&stats1, 2, 3, 3, 2, 5);
    assert_tool_model(&stats1, "mock_ai::unknown", 3, 3);

    // COMMIT 2: human deletes 1 line, AI adds 2 and deletes 3.
    let human_edit2 = "\
Line 1: Initial
H: Human Line 1
H: Human Line 2
AI: AI Line 1
AI: AI Line 2
AI: AI Line 3
Line 5: Initial
";
    repo.human_edit("example.txt", human_edit2);

    let ai_edit2 = "\
H: Human Line 2
AI: AI Line 1
AI: AI Line 3
AI: AI Line 4
AI: AI Line 5
Line 5: Initial
";
    fs::write(&file_path, ai_edit2).unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "example.txt"])
        .unwrap();

    repo.git(&["add", "example.txt"]).unwrap();
    repo.commit("Commit 2: Human deletes 1, AI adds 2 and deletes 3")
        .unwrap();
    let head_sha = repo.git(&["rev-parse", "HEAD"]).unwrap().trim().to_string();
    let stats2 = head_stats(&repo);
    assert_stats(&stats2, 0, 2, 2, 4, 2);
    assert_tool_model(&stats2, "mock_ai::unknown", 2, 2);

    // Squash the feature branch onto main via raw git, then run the CI rewrite
    // (the unified path that replaced `git-ai squash-authorship`).
    repo.git_og(&["checkout", "main"]).unwrap();
    repo.git_og(&["merge", "--squash", "feature"]).unwrap();
    repo.git_og(&[
        "commit",
        "-m",
        "Squashed: combined changes from both commits",
    ])
    .unwrap();
    let squashed_sha = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();

    let output = repo
        .git_ai(&[
            "ci",
            "local",
            "merge",
            "--merge-commit-sha",
            squashed_sha.as_str(),
            "--base-ref",
            "main",
            "--head-ref",
            "feature",
            "--head-sha",
            head_sha.as_str(),
            "--base-sha",
            base_sha.as_str(),
            "--skip-fetch",
            "--skip-push",
        ])
        .expect("ci local merge should succeed");
    assert!(
        output.contains("authorship rewritten successfully"),
        "expected ci local merge to rewrite authorship, got: {output}"
    );

    // The squashed commit carries both AI and human attribution.
    let blame_after = repo.git_ai(&["blame", "example.txt"]).unwrap();
    assert!(
        blame_after.contains("mock_ai"),
        "squashed blame should contain 'mock_ai', got:\n{blame_after}"
    );
    assert!(
        blame_after.contains("Test User"),
        "squashed blame should contain 'Test User', got:\n{blame_after}"
    );

    // Squashed final state (6 lines): "H: Human Line 2" is human; all four
    // surviving AI lines ("AI: AI Line 1/3/4/5") retain AI attribution; the
    // trailing "Line 5: Initial" is unchanged base context (committer).
    //
    // `git-ai ci local merge` routes a squash merge through the SAME
    // handle_squash_merge path the local daemon uses (union of every source
    // commit's note), so CI attribution matches the daemon: ai=4. Of the net
    // 5 added lines, 4 are AI and 1 is the human line.
    let squashed_stats = commit_stats(&repo, &["stats", &squashed_sha, "--json"]);
    assert_stats(&squashed_stats, 1, 4, 4, 4, 5);
    assert_tool_model(&squashed_stats, "mock_ai::unknown", 4, 4);
}
