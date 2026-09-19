#[macro_use]
#[path = "integration/repos/mod.rs"]
mod repos;

use repos::test_file::ExpectedLineExt;
use repos::test_repo::TestRepo;
use std::fs;

fn checkpoint_write(repo: &TestRepo, path: &str, text: &str, kind: &str) {
    repo.git_ai(&["checkpoint", "human", path]).unwrap();
    fs::write(repo.path().join(path), text).unwrap();
    repo.git_ai(&["checkpoint", kind, path]).unwrap();
}

fn assert_files(repo: &TestRepo, present: [bool; 3]) {
    for (index, has_ai) in present.into_iter().enumerate() {
        let path = format!("file-{index}.txt");
        let mut expected = lines!["base".human()];
        if has_ai {
            expected.push("original AI".ai());
        }
        repo.filename(&path).assert_committed_lines(expected);
    }
}

#[test]
fn multi_revert_restores_historical_ai_at_every_destination() {
    check_multi_revert(TestRepo::new());
}

#[test]
fn sqlite_multi_revert_restores_historical_ai_at_every_destination() {
    let repo = TestRepo::new_with_daemon_env_and_patch(&[], |patch| {
        patch.notes_backend = Some(git_ai::config::NotesBackendConfig {
            kind: git_ai::config::NotesBackendKind::Sqlite,
            backend_url: None,
        });
    });
    check_multi_revert(repo);
}

fn commit_all(repo: &TestRepo, message: &str) -> String {
    repo.git(&["add", "--all"]).unwrap();
    repo.git(&["commit", "-m", message]).unwrap();
    repo.git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

fn check_multi_revert(repo: TestRepo) {
    for i in 0..3 {
        checkpoint_write(
            &repo,
            &format!("file-{i}.txt"),
            "base\n",
            "mock_known_human",
        );
    }
    commit_all(&repo, "base");
    assert_files(&repo, [false; 3]);
    for i in 0..3 {
        checkpoint_write(
            &repo,
            &format!("file-{i}.txt"),
            "base\noriginal AI\n",
            "mock_ai",
        );
    }
    commit_all(&repo, "AI additions");
    assert_files(&repo, [true; 3]);
    let mut deleted = Vec::new();
    let mut present = [true; 3];
    for i in 0..3 {
        checkpoint_write(
            &repo,
            &format!("file-{i}.txt"),
            "base\n",
            "mock_known_human",
        );
        deleted.push(commit_all(&repo, "delete AI line"));
        present[i] = false;
        assert_files(&repo, present);
    }
    let before = deleted.last().unwrap();
    repo.git(&["revert", "--no-edit", &deleted[0], &deleted[1], &deleted[2]])
        .unwrap();
    let restored = repo
        .git_og(&["rev-list", "--reverse", &format!("{before}..HEAD")])
        .unwrap();
    let restored: Vec<_> = restored.lines().collect();
    assert_eq!(restored.len(), 3);
    for (index, oid) in restored.iter().enumerate() {
        repo.git_og(&["checkout", "--detach", oid]).unwrap();
        present[index] = true;
        assert_files(&repo, present);
    }
}

#[test]
fn revert_does_not_resurrect_ai_after_identical_untracked_recreation() {
    let repo = TestRepo::new();
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    checkpoint_write(&repo, "file.txt", "base\nsame bytes\n", "mock_ai");
    repo.stage_all_and_commit("original AI").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "same bytes".ai()]);
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    repo.stage_all_and_commit("discard AI").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    fs::write(repo.path().join("file.txt"), "base\nsame bytes\n").unwrap();
    repo.stage_all_and_commit("recreate without checkpoint")
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "same bytes".unattributed_human()]);
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    let deleted = repo
        .stage_all_and_commit("delete untracked line")
        .unwrap()
        .commit_sha;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    repo.git(&["revert", "--no-edit", &deleted]).unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "same bytes".unattributed_human()]);
}

#[test]
fn revert_follows_historical_rename_before_restoring_ai() {
    let repo = TestRepo::new();
    checkpoint_write(&repo, "old.txt", "base\n", "mock_known_human");
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("old.txt")
        .assert_committed_lines(lines!["base".human()]);
    checkpoint_write(&repo, "old.txt", "base\noriginal AI\n", "mock_ai");
    repo.stage_all_and_commit("AI addition").unwrap();
    repo.filename("old.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    repo.git(&["mv", "old.txt", "renamed.txt"]).unwrap();
    let rename = repo.commit("rename").unwrap().commit_sha;
    repo.filename("renamed.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    // Force historical reconstruction even if post-commit recorded the rename.
    let _ = repo.git_og(&["notes", "--ref=ai", "remove", &rename]);
    checkpoint_write(&repo, "other.txt", "unrelated\n", "mock_known_human");
    repo.stage_all_and_commit("unrelated change").unwrap();
    repo.filename("renamed.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["unrelated".human()]);
    checkpoint_write(&repo, "renamed.txt", "base\n", "mock_known_human");
    let deletion = repo
        .stage_all_and_commit("delete historical AI")
        .unwrap()
        .commit_sha;
    repo.filename("renamed.txt")
        .assert_committed_lines(lines!["base".human()]);
    repo.git(&["revert", "--no-edit", &deletion]).unwrap();
    repo.filename("renamed.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["unrelated".human()]);
}

#[test]
fn revert_stops_historical_inheritance_at_malformed_note() {
    let repo = TestRepo::new();
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    checkpoint_write(&repo, "file.txt", "base\noriginal AI\n", "mock_ai");
    repo.stage_all_and_commit("AI addition").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    checkpoint_write(&repo, "other.txt", "unrelated\n", "mock_known_human");
    let boundary = repo
        .stage_all_and_commit("unreadable attribution boundary")
        .unwrap()
        .commit_sha;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    repo.filename("other.txt")
        .assert_committed_lines(lines!["unrelated".human()]);
    repo.git_og(&[
        "notes",
        "--ref=ai",
        "add",
        "-f",
        "-m",
        "malformed",
        &boundary,
    ])
    .unwrap();
    repo.git(&["commit", "--allow-empty", "-m", "advance beyond boundary"])
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    let deletion = repo.stage_all_and_commit("delete AI").unwrap().commit_sha;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    repo.git(&["revert", "--no-edit", &deletion]).unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".unattributed_human()]);
}

#[test]
fn revert_preserves_existing_destination_human_attribution() {
    let gates = tempfile::tempdir().unwrap();
    let gate = gates.path().join("revert-gate");
    fs::write(&gate, "").unwrap();
    let spec = format!("revert={}", gate.display());
    let repo =
        TestRepo::new_with_daemon_env(&[("GIT_AI_TEST_SIDE_EFFECT_GATE_FOR_COMMAND", &spec)]);
    checkpoint_write(
        &repo,
        "file.txt",
        "base\noriginal human\n",
        "mock_known_human",
    );
    let mut existing = repo
        .stage_all_and_commit("human base")
        .unwrap()
        .authorship_log;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original human".human()]);
    checkpoint_write(&repo, "file.txt", "base\noriginal AI\n", "mock_ai");
    repo.stage_all_and_commit("AI replacement").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    let deletion = repo.stage_all_and_commit("delete AI").unwrap().commit_sha;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    repo.git_without_test_sync_for_test(&["revert", "--no-edit", &deletion], &[])
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !gate.with_extension("entered").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "revert did not reach side-effect gate"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let destination = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    existing.metadata.base_commit_sha = destination.clone();
    let expected = existing.clone();
    let note_file = gates.path().join("destination-note");
    fs::write(&note_file, existing.serialize_to_string().unwrap()).unwrap();
    repo.git_og(&[
        "notes",
        "--ref=ai",
        "add",
        "-f",
        "-F",
        note_file.to_str().unwrap(),
        &destination,
    ])
    .unwrap();
    fs::remove_file(&gate).unwrap();
    repo.sync_daemon_force();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".human()]);
    let actual = repo.require_authorship_log(&destination);
    assert_eq!(actual.attestations, expected.attestations);
    assert_eq!(actual.metadata.humans, expected.metadata.humans);
}

#[test]
fn revert_does_not_infer_history_through_a_merge_without_a_note() {
    let repo = TestRepo::new();
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    repo.stage_all_and_commit("base").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    checkpoint_write(&repo, "file.txt", "base\noriginal AI\n", "mock_ai");
    repo.stage_all_and_commit("AI addition").unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    let main = repo.current_branch();
    repo.git(&["checkout", "-b", "side"]).unwrap();
    checkpoint_write(&repo, "side.txt", "side\n", "mock_known_human");
    repo.stage_all_and_commit("side change").unwrap();
    repo.filename("side.txt")
        .assert_committed_lines(lines!["side".human()]);
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    repo.git(&["checkout", &main]).unwrap();
    checkpoint_write(&repo, "main.txt", "main\n", "mock_known_human");
    repo.stage_all_and_commit("main change").unwrap();
    repo.filename("main.txt")
        .assert_committed_lines(lines!["main".human()]);
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    repo.git(&["merge", "--no-ff", "--no-edit", "side"])
        .unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".ai()]);
    let merge = repo
        .git_og(&["rev-parse", "HEAD"])
        .unwrap()
        .trim()
        .to_string();
    let _ = repo.git_og(&["notes", "--ref=ai", "remove", &merge]);
    checkpoint_write(&repo, "file.txt", "base\n", "mock_known_human");
    let deletion = repo.stage_all_and_commit("delete AI").unwrap().commit_sha;
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human()]);
    repo.git(&["revert", "--no-edit", &deletion]).unwrap();
    repo.filename("file.txt")
        .assert_committed_lines(lines!["base".human(), "original AI".unattributed_human()]);
}
