use crate::repos::test_file::ExpectedLineExt;
use crate::repos::test_repo::TestRepo;
use git_ai::model::authorship_log_serialization::AuthorshipLog;

#[test]
#[cfg(unix)]
fn immutable_rebase_preserves_65_leading_squash_sources() {
    large_squash(false);
}

#[test]
#[cfg(unix)]
fn continued_rebase_retains_original_immutable_squash_boundary() {
    large_squash(true);
}

#[cfg(unix)]
fn large_squash(conflict: bool) {
    use std::fs;

    let repo = TestRepo::new();
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base".human()]);
    let base_commit = repo.stage_all_and_commit("Base").unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    let mut feature = repo.filename("feature.txt");
    let expected_feature = |count| {
        (1..=count)
            .map(|n| {
                let line = format!("feature {n}");
                if n == 1 {
                    line.ai()
                } else {
                    line.unattributed_human()
                }
            })
            .collect::<Vec<_>>()
    };
    for count in 1..=65 {
        fs::write(
            repo.path().join("feature.txt"),
            (1..=count)
                .map(|n| format!("feature {n}\n"))
                .collect::<String>(),
        )
        .unwrap();
        if count == 1 {
            repo.git_ai(&["checkpoint", "mock_ai", "feature.txt"])
                .unwrap();
        }
        repo.stage_all_and_commit(&format!("Feature {count}"))
            .unwrap();
        base.assert_committed_lines(crate::lines!["base".human()]);
        feature.assert_committed_lines(expected_feature(count));
    }
    let mut anchor = repo.filename("anchor.txt");
    fs::write(
        repo.path().join("anchor.txt"),
        (1..=1000)
            .map(|n| format!("anchor {n}\n"))
            .collect::<String>(),
    )
    .unwrap();
    repo.git_ai(&["checkpoint", "mock_ai", "anchor.txt"])
        .unwrap();
    let old_tip = repo.stage_all_and_commit("Dominant final commit").unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    feature.assert_committed_lines(expected_feature(65));
    anchor.assert_committed_lines((1..=1000).map(|n| format!("anchor {n}").ai()).collect());

    // The final patch dominates similarity, making the other 65 commits a
    // leading unmatched group rather than an ordinary post-match squash.
    let editor = repo.path().join(".git/large-squash-editor.sh");
    crate::repos::write_executable_script(
        &editor,
        r#"#!/bin/sh
last="$(grep '^pick ' "$1" | tail -n 1)"
{
  printf '%s\n' "$last"
  grep '^pick ' "$1" | sed '$d; s/^pick /fixup /'
} > "$1.tmp"
mv "$1.tmp" "$1"
"#,
    )
    .unwrap();
    let editor_env = [
        ("GIT_SEQUENCE_EDITOR", editor.to_str().unwrap()),
        ("GIT_EDITOR", "true"),
    ];
    let new_base = if conflict {
        let branch = repo.current_branch();
        repo.git(&["checkout", "-b", "new-base", &base_commit.commit_sha])
            .unwrap();
        anchor.set_contents(crate::lines!["conflicting base".human()]);
        let commit = repo
            .stage_all_and_commit("Conflicting destination")
            .unwrap();
        base.assert_committed_lines(crate::lines!["base".human()]);
        anchor.assert_committed_lines(crate::lines!["conflicting base".human()]);
        repo.git(&["checkout", &branch]).unwrap();
        let failure = repo
            .git_with_env(
                &[
                    "rebase",
                    "-i",
                    "--onto",
                    &commit.commit_sha,
                    &base_commit.commit_sha,
                ],
                &editor_env,
                None,
            )
            .unwrap_err();
        assert_eq!(
            repo.git(&["diff", "--name-only", "--diff-filter=U"])
                .unwrap()
                .trim(),
            "anchor.txt",
            "expected anchor conflict: {failure}"
        );
        fs::write(
            repo.path().join("anchor.txt"),
            (1..=1000)
                .map(|n| format!("anchor {n}\n"))
                .collect::<String>(),
        )
        .unwrap();
        repo.git(&["add", "anchor.txt"]).unwrap();
        repo.git_with_env(&["rebase", "--continue"], &editor_env, None)
            .unwrap();
        commit.commit_sha
    } else {
        repo.git_with_env(&["rebase", "-i", "HEAD~66"], &editor_env, None)
            .unwrap();
        base_commit.commit_sha.clone()
    };
    let range_diff = repo
        .git(&[
            "range-diff",
            "-s",
            "--creation-factor=100",
            &format!("{}..{}", base_commit.commit_sha, old_tip.commit_sha),
            &format!("{new_base}..HEAD"),
        ])
        .unwrap();
    let leading_drops = range_diff
        .lines()
        .map(|line| line.split_whitespace().nth(2).unwrap_or(""))
        .take_while(|status| !matches!(*status, "=" | "!"))
        .filter(|status| *status == "<")
        .count();
    assert_eq!(
        leading_drops, 65,
        "fixture must exceed the generic bound:\n{range_diff}"
    );
    base.assert_committed_lines(crate::lines!["base".human()]);
    feature.assert_committed_lines(expected_feature(65));
    anchor.assert_committed_lines((1..=1000).map(|n| format!("anchor {n}").ai()).collect());
}

#[test]
fn ambiguous_onto_rebase_does_not_copy_old_upstream_authorship() {
    let repo = TestRepo::new();
    let mut base = repo.filename("base.txt");
    base.set_contents(crate::lines!["base".human()]);
    let root = repo.stage_all_and_commit("Base").unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    repo.git(&["checkout", "-b", "old-upstream"]).unwrap();
    let mut shared = repo.filename("shared.txt");
    shared.set_contents(crate::lines!["shared content".ai()]);
    repo.stage_all_and_commit("Old upstream").unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    shared.assert_committed_lines(crate::lines!["shared content".ai()]);
    repo.git(&["checkout", "-b", "feature"]).unwrap();
    let mut feature = repo.filename("feature.txt");
    feature.set_contents(crate::lines!["feature ai".ai()]);
    repo.stage_all_and_commit("Feature").unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    shared.assert_committed_lines(crate::lines!["shared content".ai()]);
    feature.assert_committed_lines(crate::lines!["feature ai".ai()]);
    repo.git(&["checkout", "-b", "new-base", &root.commit_sha])
        .unwrap();
    shared.set_contents(crate::lines!["shared content".human()]);
    repo.stage_all_and_commit("New base").unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    shared.assert_committed_lines(crate::lines!["shared content".human()]);
    repo.git(&["checkout", "feature"]).unwrap();
    repo.git(&["rebase", "--onto", "new-base", "old-upstream"])
        .unwrap();
    base.assert_committed_lines(crate::lines!["base".human()]);
    shared.assert_committed_lines(crate::lines!["shared content".human()]);
    feature.assert_committed_lines(crate::lines!["feature ai".ai()]);
    let tip = repo.git(&["rev-parse", "HEAD"]).unwrap();
    let note = repo.read_authorship_note(tip.trim()).unwrap();
    let log = AuthorshipLog::deserialize_from_string(&note).unwrap();
    assert!(
        log.attestations.iter().all(|a| a.file_path != "shared.txt"),
        "unrelated old-upstream note leaked: {:?}",
        log.attestations
    );
}
