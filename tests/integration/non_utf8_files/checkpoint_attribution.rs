use super::{
    CommitStats, ExpectedLineExt, TestRepo, extract_json_object, fs, gbk_hello_world,
    gbk_multiline, latin1_bytes, shift_jis_bytes,
};

// =============================================================================
// AI attribution: UTF-8 files get correct attribution even with non-UTF-8 neighbors
// =============================================================================

#[test]
fn test_ai_attribution_preserved_with_non_utf8_in_same_commit() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let mut ai_file = repo.filename("ai_output.py");
    ai_file.set_contents(crate::lines![
        "def hello():".ai(),
        "    return 'world'".ai(),
    ]);

    let gbk_path = repo.path().join("legacy_gbk.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    let commit = repo.stage_all_and_commit("Mixed commit").unwrap();

    assert!(
        !commit.authorship_log.attestations.is_empty(),
        "Should have attestation for the AI-written file"
    );

    let ai_attestation = commit
        .authorship_log
        .attestations
        .iter()
        .find(|a| a.file_path == "ai_output.py");
    assert!(
        ai_attestation.is_some(),
        "Should have attestation specifically for ai_output.py"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(
        stats.ai_additions, 2,
        "AI additions should be correctly counted for the UTF-8 file"
    );
}

#[test]
fn test_human_and_ai_edits_with_non_utf8_file_present() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let gbk_path = repo.path().join("legacy.txt");
    fs::write(&gbk_path, gbk_hello_world()).unwrap();
    repo.stage_all_and_commit("Add legacy GBK file").unwrap();

    let mut code_file = repo.filename("app.js");
    code_file.set_contents(crate::lines![
        "const a = 1;".human(),
        "const b = generateCode();".ai(),
        "const c = 3;".human(),
    ]);
    repo.stage_all_and_commit("Add code file").unwrap();

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert_eq!(stats.ai_additions, 1, "1 AI line should be counted");
    assert_eq!(stats.human_additions, 2, "2 human lines should be counted");
}

// =============================================================================
// Checkpoint: Non-UTF-8 files during checkpoint
// =============================================================================

#[test]
fn test_checkpoint_with_non_utf8_file() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let file_path = repo.path().join("gbk_file.txt");
    fs::write(&file_path, gbk_multiline()).unwrap();

    let result = repo.git_ai(&["checkpoint"]);
    assert!(
        result.is_ok(),
        "Checkpoint with non-UTF-8 file should not crash, got: {:?}",
        result.err()
    );
}

#[test]
fn test_checkpoint_ai_with_non_utf8_file_present() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let gbk_path = repo.path().join("legacy.txt");
    fs::write(&gbk_path, gbk_hello_world()).unwrap();
    repo.git(&["add", "-A"]).unwrap();
    repo.git_ai(&["checkpoint"]).unwrap();

    let mut ai_file = repo.filename("generated.py");
    ai_file.set_contents(crate::lines!["print('hello')".ai(), "print('world')".ai()]);

    let commit = repo
        .stage_all_and_commit("Commit with AI file and GBK file")
        .unwrap();

    let ai_attestation = commit
        .authorship_log
        .attestations
        .iter()
        .find(|a| a.file_path == "generated.py");
    assert!(
        ai_attestation.is_some(),
        "AI file should still get proper attestation"
    );
}

// =============================================================================
// Binary files: Should be handled gracefully (related edge case)
// =============================================================================

#[test]
fn test_binary_file_does_not_crash_commit() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let binary_path = repo.path().join("image.png");
    let binary_content: Vec<u8> = vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG header
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1 pixel
        0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE,
    ];
    fs::write(&binary_path, binary_content).unwrap();

    let result = repo.stage_all_and_commit("Add binary file");
    assert!(
        result.is_ok(),
        "Committing a binary file should not fail, got: {:?}",
        result.err()
    );
}

#[test]
fn test_binary_and_non_utf8_with_ai_file() {
    let repo = TestRepo::new();

    let mut readme = repo.filename("README.md");
    readme.set_contents(crate::lines!["# Project"]);
    repo.stage_all_and_commit("Initial commit").unwrap();

    let binary_path = repo.path().join("data.bin");
    fs::write(&binary_path, vec![0x00, 0x01, 0x02, 0xFF, 0xFE]).unwrap();

    let gbk_path = repo.path().join("chinese.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    let mut ai_file = repo.filename("output.rs");
    ai_file.set_contents(crate::lines![
        "fn process() -> bool {".ai(),
        "    true".ai(),
        "}".ai(),
    ]);

    let commit = repo
        .stage_all_and_commit("Add binary, GBK, and AI files")
        .unwrap();

    let ai_attestation = commit
        .authorship_log
        .attestations
        .iter()
        .find(|a| a.file_path == "output.rs");
    assert!(
        ai_attestation.is_some(),
        "AI file should get attestation even with binary and non-UTF-8 neighbors"
    );

    let raw = repo.git_ai(&["stats", "--json"]).unwrap();
    let json = extract_json_object(&raw);
    let stats: CommitStats = serde_json::from_str(&json).unwrap();

    assert!(
        stats.ai_additions >= 3,
        "AI additions should include at least the 3 AI-attributed lines, got: {}",
        stats.ai_additions
    );
}

// =============================================================================
// Line-level attribution: Prove per-line AI/human attribution is correct
// with non-UTF-8 files present
// =============================================================================

#[test]
fn test_line_attribution_ai_file_with_gbk_neighbor() {
    let repo = TestRepo::new();
    let mut ai_file = repo.filename("code.py");

    ai_file.set_contents(crate::lines![
        "def greet():".ai(),
        "    return 'hello'".ai(),
        "# human comment",
    ]);

    let gbk_path = repo.path().join("chinese.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    repo.stage_all_and_commit("Add AI code and GBK file")
        .unwrap();

    ai_file.assert_lines_and_blame(crate::lines![
        "def greet():".ai(),
        "    return 'hello'".ai(),
        "# human comment".human(),
    ]);
}

#[test]
fn test_line_attribution_multi_commit_with_non_utf8_neighbor() {
    let repo = TestRepo::new();
    let mut file = repo.filename("app.ts");

    file.set_contents(crate::lines!["const x = 1;", "const y = 2;"]);

    let gbk_path = repo.path().join("legacy.txt");
    fs::write(&gbk_path, gbk_hello_world()).unwrap();

    repo.stage_all_and_commit("Base commit with GBK neighbor")
        .unwrap();

    file.insert_at(
        2,
        crate::lines![
            "const ai_z = compute();".ai(),
            "const ai_w = transform();".ai()
        ],
    );

    fs::write(&gbk_path, gbk_multiline()).unwrap();

    repo.stage_all_and_commit("AI additions alongside GBK edit")
        .unwrap();

    file.assert_lines_and_blame(crate::lines![
        "const x = 1;".human(),
        "const y = 2;".ai(),
        "const ai_z = compute();".ai(),
        "const ai_w = transform();".ai(),
    ]);
}

#[test]
fn test_line_attribution_interleaved_ai_human_with_non_utf8() {
    let repo = TestRepo::new();
    let mut file = repo.filename("mixed.rs");

    file.set_contents(crate::lines!["fn main() {"]);

    let latin1_path = repo.path().join("notes.txt");
    fs::write(&latin1_path, latin1_bytes()).unwrap();

    repo.stage_all_and_commit("Base commit").unwrap();

    file.insert_at(
        1,
        crate::lines![
            "    let a = ai_gen();".ai(),
            "    let b = human_wrote();".human(),
            "    let c = ai_gen_2();".ai(),
        ],
    );

    repo.stage_all_and_commit("Interleaved additions").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "fn main() {".ai(),
        "    let a = ai_gen();".ai(),
        "    let b = human_wrote();".ai(),
        "    let c = ai_gen_2();".ai(),
    ]);
}

#[test]
fn test_line_attribution_ai_replaces_lines_with_non_utf8_present() {
    let repo = TestRepo::new();
    let mut file = repo.filename("config.js");

    file.set_contents(crate::lines![
        "const a = 1;",
        "const b = 2;",
        "const c = 3;",
        "const d = 4;",
    ]);

    let sjis_path = repo.path().join("japanese.txt");
    fs::write(&sjis_path, shift_jis_bytes()).unwrap();

    repo.stage_all_and_commit("Initial commit").unwrap();

    file.replace_at(1, "const b = ai_replacement();".ai());
    file.replace_at(2, "const c = ai_replacement_2();".ai());

    repo.stage_all_and_commit("AI replaces middle lines")
        .unwrap();

    file.assert_lines_and_blame(crate::lines![
        "const a = 1;".human(),
        "const b = ai_replacement();".ai(),
        "const c = ai_replacement_2();".ai(),
        "const d = 4;".human(),
    ]);
}

#[test]
fn test_line_attribution_multiple_utf8_files_with_non_utf8_neighbors() {
    let repo = TestRepo::new();
    let mut file_a = repo.filename("module_a.py");
    let mut file_b = repo.filename("module_b.py");

    file_a.set_contents(crate::lines![
        "def func_a():".ai(),
        "    pass".ai(),
        "# end of a",
    ]);

    file_b.set_contents(crate::lines![
        "def func_b():".human(),
        "    return ai_result()".ai(),
    ]);

    let gbk_path = repo.path().join("data_gbk.txt");
    fs::write(&gbk_path, gbk_multiline()).unwrap();

    let latin1_path = repo.path().join("data_latin1.txt");
    fs::write(&latin1_path, latin1_bytes()).unwrap();

    repo.stage_all_and_commit("Add multiple files with non-UTF-8 neighbors")
        .unwrap();

    file_a.assert_lines_and_blame(crate::lines![
        "def func_a():".ai(),
        "    pass".ai(),
        "# end of a".human(),
    ]);

    file_b.assert_lines_and_blame(crate::lines![
        "def func_b():".human(),
        "    return ai_result()".ai(),
    ]);
}

#[test]
fn test_line_attribution_ai_across_multiple_commits_with_non_utf8() {
    let repo = TestRepo::new();
    let mut file = repo.filename("evolving.ts");

    file.set_contents(crate::lines!["const base = true;", ""]);

    let gbk_path = repo.path().join("persistent_gbk.txt");
    fs::write(&gbk_path, gbk_hello_world()).unwrap();

    repo.stage_all_and_commit("Base commit").unwrap();

    file.insert_at(
        1,
        crate::lines!["const first_ai = 1;".ai(), "const second_ai = 2;".ai()],
    );

    fs::write(&gbk_path, gbk_multiline()).unwrap();

    repo.stage_all_and_commit("First AI batch").unwrap();

    file.insert_at(
        3,
        crate::lines!["const third_ai = 3;".ai(), "const fourth_ai = 4;".ai()],
    );

    repo.stage_all_and_commit("Second AI batch").unwrap();

    file.assert_lines_and_blame(crate::lines![
        "const base = true;".human(),
        "const first_ai = 1;".ai(),
        "const second_ai = 2;".ai(),
        "const third_ai = 3;".ai(),
        "const fourth_ai = 4;".ai(),
    ]);
}

crate::reuse_tests_in_worktree!(
    test_ai_attribution_preserved_with_non_utf8_in_same_commit,
    test_human_and_ai_edits_with_non_utf8_file_present,
    test_checkpoint_with_non_utf8_file,
    test_checkpoint_ai_with_non_utf8_file_present,
    test_binary_file_does_not_crash_commit,
    test_binary_and_non_utf8_with_ai_file,
    test_line_attribution_ai_file_with_gbk_neighbor,
    test_line_attribution_multi_commit_with_non_utf8_neighbor,
    test_line_attribution_interleaved_ai_human_with_non_utf8,
    test_line_attribution_ai_replaces_lines_with_non_utf8_present,
    test_line_attribution_multiple_utf8_files_with_non_utf8_neighbors,
    test_line_attribution_ai_across_multiple_commits_with_non_utf8,
);
