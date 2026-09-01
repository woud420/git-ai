use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use std::fs;

// Regression coverage for ENG-316 and upstream git-ai-project/git-ai#2176.
struct DivergedWorktree {
    repo: TestRepo,
    primary_commit: String,
    linked_commit: String,
    linked_prompt_id: String,
}

fn diverged_worktree_with_lowercase_head() -> DivergedWorktree {
    let repo = TestRepo::new_worktree();
    let mut file = repo.filename("test.txt");

    file.set_contents(crate::lines!["primary AI line".ai()]);
    let primary = repo.stage_all_and_commit("Primary branch commit").unwrap();
    file.assert_committed_lines(crate::lines!["primary AI line".ai()]);

    // Keep the primary worktree's branch at the first attributed commit while
    // advancing the linked worktree's branch to a distinct attributed commit.
    repo.git(&["update-ref", "refs/heads/main", &primary.commit_sha])
        .unwrap();
    file.insert_at(1, crate::lines!["linked AI line".ai()]);
    let linked = repo.stage_all_and_commit("Linked worktree commit").unwrap();
    file.assert_committed_lines(crate::lines!["primary AI line".ai(), "linked AI line".ai(),]);

    let linked_prompt_id = linked
        .authorship_log
        .metadata
        .sessions
        .keys()
        .next()
        .expect("linked commit should contain an AI session")
        .clone();

    // On a case-insensitive filesystem, `head` resolves through the common
    // Git directory's HEAD. Create that spelling explicitly so the macOS bug
    // is reproduced on every test platform.
    let common_dir = repo.git(&["rev-parse", "--git-common-dir"]).unwrap();
    let common_dir = repo.path().join(common_dir.trim());
    let lowercase_head = common_dir.join("head");
    if !lowercase_head.exists() {
        fs::copy(common_dir.join("HEAD"), lowercase_head).unwrap();
    }

    assert_eq!(
        repo.git(&["rev-parse", "head"]).unwrap().trim(),
        primary.commit_sha,
        "lowercase head should resolve through the primary worktree"
    );
    assert_eq!(
        repo.git(&["rev-parse", "HEAD"]).unwrap().trim(),
        linked.commit_sha,
        "uppercase HEAD should resolve through the linked worktree"
    );

    DivergedWorktree {
        repo,
        primary_commit: primary.commit_sha,
        linked_commit: linked.commit_sha,
        linked_prompt_id,
    }
}

fn record_parity(
    name: impl Into<String>,
    lowercase: Result<String, String>,
    uppercase: Result<String, String>,
    matching: &mut Vec<String>,
    mismatching: &mut Vec<String>,
) {
    let name = name.into();
    match (lowercase, uppercase) {
        (Ok(lowercase), Ok(uppercase)) if lowercase == uppercase => matching.push(name),
        (lowercase, uppercase) => mismatching.push(format!(
            "{name}: lowercase={lowercase:?}, uppercase={uppercase:?}"
        )),
    }
}

#[test]
fn user_revision_commands_normalize_lowercase_head_in_linked_worktree() {
    // Regression coverage for ENG-316.
    let fixture = diverged_worktree_with_lowercase_head();
    let repo = &fixture.repo;
    let mut matching = Vec::new();
    let mut mismatching = Vec::new();

    record_parity(
        "show",
        repo.git_ai(&["show", "head"]),
        repo.git_ai(&["show", "HEAD"]),
        &mut matching,
        &mut mismatching,
    );
    record_parity(
        "diff",
        repo.git_ai(&["diff", "head"]),
        repo.git_ai(&["diff", "HEAD"]),
        &mut matching,
        &mut mismatching,
    );
    record_parity(
        "stats",
        repo.git_ai(&["stats", "head", "--json"]),
        repo.git_ai(&["stats", "HEAD", "--json"]),
        &mut matching,
        &mut mismatching,
    );
    record_parity(
        "show-prompt",
        repo.git_ai(&["show-prompt", &fixture.linked_prompt_id, "--commit", "head"]),
        repo.git_ai(&["show-prompt", &fixture.linked_prompt_id, "--commit", "HEAD"]),
        &mut matching,
        &mut mismatching,
    );

    assert!(
        mismatching.is_empty(),
        "lowercase HEAD diverged for {mismatching:?}; commands already matching: {matching:?}"
    );
}

#[test]
fn ranges_normalize_both_endpoints_and_preserve_head_suffixes() {
    // Regression coverage for ENG-316.
    let fixture = diverged_worktree_with_lowercase_head();
    let repo = &fixture.repo;
    let mut matching = Vec::new();
    let mut mismatching = Vec::new();

    for (name, lowercase, uppercase) in [
        (
            "show range",
            repo.git_ai(&["show", "head~1..head"]),
            repo.git_ai(&["show", "HEAD~1..HEAD"]),
        ),
        (
            "diff range",
            repo.git_ai(&["diff", "head~1..head"]),
            repo.git_ai(&["diff", "HEAD~1..HEAD"]),
        ),
        (
            "stats range",
            repo.git_ai(&["stats", "head~1..head", "--json"]),
            repo.git_ai(&["stats", "HEAD~1..HEAD", "--json"]),
        ),
    ] {
        record_parity(name, lowercase, uppercase, &mut matching, &mut mismatching);
    }

    for (lowercase, uppercase) in [
        ("head~1", "HEAD~1"),
        ("head^1", "HEAD^1"),
        ("head@{0}", "HEAD@{0}"),
    ] {
        record_parity(
            format!("show suffix {lowercase}"),
            repo.git_ai(&["show", lowercase]),
            repo.git_ai(&["show", uppercase]),
            &mut matching,
            &mut mismatching,
        );
    }

    assert!(
        mismatching.is_empty(),
        "lowercase HEAD ranges or suffixes diverged for {mismatching:?}; already matching: {matching:?}"
    );
}

#[test]
fn revision_names_that_merely_begin_with_head_are_not_rewritten() {
    // Regression coverage for ENG-316.
    let fixture = diverged_worktree_with_lowercase_head();
    let repo = &fixture.repo;

    for branch in ["header", "head@topic"] {
        repo.git(&["branch", branch, &fixture.primary_commit])
            .unwrap();
        assert_eq!(
            repo.git_ai(&["show", branch]).unwrap(),
            repo.git_ai(&["show", &fixture.primary_commit]).unwrap(),
            "branch `{branch}` must remain intact"
        );
    }

    assert_ne!(fixture.primary_commit, fixture.linked_commit);
    assert_ne!(
        repo.git_ai(&["show", "head@topic"]).unwrap(),
        repo.git_ai(&["show", "HEAD"]).unwrap(),
        "head@topic must resolve to the branch, not linked-worktree HEAD"
    );
}

#[test]
fn non_ascii_revision_names_are_not_rewritten_or_sliced_mid_character() {
    // Regression coverage for ENG-316.
    let fixture = diverged_worktree_with_lowercase_head();
    let repo = &fixture.repo;
    repo.git(&["branch", "中文分支", &fixture.primary_commit])
        .unwrap();

    assert_eq!(
        repo.git_ai(&["show", "中文分支"]).unwrap(),
        repo.git_ai(&["show", &fixture.primary_commit]).unwrap()
    );
}
