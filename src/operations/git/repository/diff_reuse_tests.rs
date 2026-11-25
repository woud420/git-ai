use super::*;
use crate::operations::commands::diff_parsing::get_diff_text;
use crate::operations::git::test_utils::TmpRepo;

/// Whether the full diff was reused for `paths`. Whenever it is, the result
/// must equal what git gives for the same diff restricted to `paths`.
fn reused(tmp: &TmpRepo, base: &str, commit: &str, paths: &[&str]) -> bool {
    let repo = tmp.gitai_repo();
    let full = get_diff_text(repo, base, commit, true).unwrap();
    let pathspecs: HashSet<String> = paths.iter().map(|p| p.to_string()).collect();
    let Some(added) = added_lines_if_pathspecs_cover(&full, &pathspecs) else {
        return false;
    };
    let from_git = repo
        .diff_added_lines(base, commit, Some(&pathspecs))
        .unwrap();
    assert_eq!(added, from_git, "{paths:?} at {commit}");
    true
}

#[test]
fn full_diff_is_reused_only_when_git_would_print_the_same() {
    let tmp = TmpRepo::new().unwrap();
    let git = |args: &[&str]| tmp.git_command(args).unwrap().trim().to_string();
    tmp.write_file("a.txt", "a\n", false).unwrap();
    tmp.write_file("b.txt", "b1\nb2\nb3\nb4\nb5\nb6\n", false)
        .unwrap();
    tmp.write_file("sp ace.txt", "s\n", false).unwrap();
    tmp.write_file("sub/c.rs", "fn c() {}\n", false).unwrap();
    std::fs::write(tmp.path().join("bin.dat"), b"\x00\x01").unwrap();
    let root = tmp.commit_all("root").unwrap();
    let all = ["a.txt", "b.txt", "sp ace.txt", "sub/c.rs", "bin.dat"];
    let empty = crate::operations::authorship::diff_base::empty_tree_for(&root);
    assert!(reused(&tmp, empty, &root, &all));
    assert!(!reused(&tmp, empty, &root, &all[..4]));

    // Plain edits: reused when every changed file is named exactly.
    tmp.write_file("a.txt", "a\nmore\n", false).unwrap();
    tmp.write_file("b.txt", "b1\nb2\nb3\nb4\nb5\nb6\nb7\n", false)
        .unwrap();
    let edits = tmp.commit_all("edits").unwrap();
    let base = format!("{edits}^");
    assert!(reused(&tmp, &base, &edits, &["a.txt", "b.txt"]));
    assert!(reused(&tmp, &base, &edits, &["a.txt", "b.txt", "absent"]));
    assert!(!reused(&tmp, &base, &edits, &["a.txt"]));
    assert!(!reused(&tmp, &base, &edits, &["a.txt", "b.txt", ":x"]));
    assert!(!reused(&tmp, &base, &edits, &["a.txt", "b.txt", "\u{e9}"]));
    assert!(!reused(&tmp, &base, &edits, &[]));

    // A rename: restricting the diff can turn it into an add, so it never reuses.
    git(&["mv", "b.txt", "b2.txt"]);
    tmp.write_file("b2.txt", "b1\nb2\nb3\nb4\nb5\nb6\nb7\nb8\n", false)
        .unwrap();
    tmp.write_file("sp ace.txt", "s\nt\n", false).unwrap();
    let renamed = tmp.commit_all("rename").unwrap();
    let base = format!("{renamed}^");
    assert!(!reused(
        &tmp,
        &base,
        &renamed,
        &["b.txt", "b2.txt", "sp ace.txt"]
    ));
    let full = get_diff_text(tmp.gitai_repo(), &base, &renamed, true).unwrap();
    let restricted: HashSet<String> = ["b2.txt".to_string()].into();
    assert_ne!(
        parse_diff_added_lines_internal(&full)
            .all_lines
            .get("b2.txt"),
        tmp.gitai_repo()
            .diff_added_lines(&base, &renamed, Some(&restricted))
            .unwrap()
            .get("b2.txt"),
        "a restricted diff loses the rename"
    );

    // A deletion, a binary change and a mode-only change.
    git(&["rm", "-q", "sub/c.rs"]);
    std::fs::write(tmp.path().join("bin.dat"), b"\x00\x02").unwrap();
    git(&["add", "bin.dat"]);
    git(&["update-index", "--chmod=+x", "a.txt"]);
    git(&["commit", "-q", "-m", "mixed"]);
    let mixed = git(&["rev-parse", "HEAD"]);
    let base = format!("{mixed}^");
    assert!(reused(
        &tmp,
        &base,
        &mixed,
        &["sub/c.rs", "bin.dat", "a.txt"]
    ));
    assert!(!reused(&tmp, &base, &mixed, &["sub/c.rs", "bin.dat"]));
    assert!(!reused(&tmp, &base, &mixed, &["sub", "bin.dat", "a.txt"]));
}
