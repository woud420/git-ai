use super::{
    Duration, TestRepo, add_and_commit, assert_checkpoint_contains, assert_checkpoint_excludes,
    checkpoint_paths, post_hook, pre_hook, repo_root, run_bash, snapshot, thread,
};

// ===========================================================================
// Category 3: File deletion commands
// ===========================================================================

// ===========================================================================
// Category 4: Build/compile tool simulations
// ===========================================================================

#[test]
fn test_bash_provenance_simulated_cargo_init() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "cargo-sess", "cargo-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "mkdir -p myproject/src && echo 'fn main() {}' > myproject/src/main.rs && printf '[package]\\nname=\"myproject\"' > myproject/Cargo.toml",
        ],
    );

    let post_action = post_hook(&root, "cargo-sess", "cargo-t1");
    assert_checkpoint_contains(&post_action, "main.rs");
    // On macOS, paths are case-normalized to lowercase, so check for lowercase.
    let paths = checkpoint_paths(&post_action);
    assert!(
        paths
            .iter()
            .any(|p| p.to_lowercase().contains("cargo.toml")),
        "Cargo.toml (case-insensitive) should appear in checkpoint; got {:?}",
        paths
    );
}

#[test]
fn test_bash_provenance_simulated_npm_init() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "npm-sess", "npm-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            r#"echo '{"name":"test","version":"1.0.0"}' > package.json"#,
        ],
    );

    let post_action = post_hook(&root, "npm-sess", "npm-t1");
    assert_checkpoint_contains(&post_action, "package.json");
}

// ===========================================================================
// Category 6: Multi-command pipelines
// ===========================================================================

#[test]
fn test_bash_provenance_loop_creating_multiple_files() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "loop-sess", "loop-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "for f in a.txt b.txt c.txt; do echo 'content' > $f; done",
        ],
    );

    let post_action = post_hook(&root, "loop-sess", "loop-t1");
    assert_checkpoint_contains(&post_action, "a.txt");
    assert_checkpoint_contains(&post_action, "b.txt");
    assert_checkpoint_contains(&post_action, "c.txt");
}

// ===========================================================================
// Category 9: Large/batch operations
// ===========================================================================

#[test]
fn test_bash_provenance_create_50_files() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "init.txt", "seed", "initial commit");

    pre_hook(&root, "batch50-sess", "batch50-t1");

    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "for i in $(seq 1 50); do echo \"file $i\" > \"batch_$i.txt\"; done",
        ],
    );

    let post_action = post_hook(&root, "batch50-sess", "batch50-t1");
    let paths = checkpoint_paths(&post_action);
    assert!(
        paths.len() >= 50,
        "Expected at least 50 created files in checkpoint; got {} paths: {:?}",
        paths.len(),
        paths
    );
    // Spot-check a few
    assert_checkpoint_contains(&post_action, "batch_1.txt");
    assert_checkpoint_contains(&post_action, "batch_25.txt");
    assert_checkpoint_contains(&post_action, "batch_50.txt");
}

#[test]
fn test_bash_provenance_modify_20_of_50_tracked() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);

    // Create and commit 50 files
    for i in 1..=50 {
        let name = format!("tracked_{}.txt", i);
        add_and_commit(
            &repo,
            &name,
            &format!("original {}", i),
            &format!("add {}", name),
        );
    }

    pre_hook(&root, "mod20-sess", "mod20-t1");

    thread::sleep(Duration::from_millis(50));
    // Modify only files 1-20
    run_bash(
        &repo,
        "sh",
        &[
            "-c",
            "for i in $(seq 1 20); do echo 'modified' > \"tracked_$i.txt\"; done",
        ],
    );

    let post_action = post_hook(&root, "mod20-sess", "mod20-t1");
    let paths = checkpoint_paths(&post_action);

    // Exactly 20 files should be modified
    assert_eq!(
        paths.len(),
        20,
        "Expected exactly 20 modified files; got {} paths: {:?}",
        paths.len(),
        paths
    );

    // Verify modified files are present
    assert_checkpoint_contains(&post_action, "tracked_1.txt");
    assert_checkpoint_contains(&post_action, "tracked_20.txt");

    // Verify unmodified files are NOT present
    assert_checkpoint_excludes(&post_action, "tracked_21.txt");
    assert_checkpoint_excludes(&post_action, "tracked_50.txt");
}

// ===========================================================================
// Category 11: Tar/archive operations
// ===========================================================================

#[test]
fn test_bash_provenance_create_tarball() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "archive/one.txt", "one", "add one");
    add_and_commit(&repo, "archive/two.txt", "two", "add two");

    pre_hook(&root, "tar-create-sess", "tar-create-t1");

    run_bash(&repo, "tar", &["czf", "archive.tar.gz", "archive"]);

    let post_action = post_hook(&root, "tar-create-sess", "tar-create-t1");
    assert_checkpoint_contains(&post_action, "archive.tar.gz");
}

#[test]
fn test_bash_provenance_extract_tarball() {
    use git_ai::operations::commands::checkpoint_agent::bash_tool::diff;
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(&repo, "pkg/alpha.txt", "alpha", "add alpha");
    add_and_commit(&repo, "pkg/beta.txt", "beta", "add beta");

    // Create the tarball first
    run_bash(&repo, "tar", &["czf", "pkg.tar.gz", "pkg"]);
    repo.git_og(&["add", "pkg.tar.gz"])
        .expect("git add tarball should succeed");
    repo.git_og(&["commit", "-m", "add tarball"])
        .expect("git commit tarball should succeed");

    // Remove original directory
    run_bash(&repo, "rm", &["-rf", "pkg"]);
    repo.git_og(&["add", "-A"])
        .expect("git add removal should succeed");
    repo.git_og(&["commit", "-m", "remove pkg dir"])
        .expect("git commit removal should succeed");

    let pre = snapshot(&root, "tar-extract-sess", "tar-extract-t1", None).unwrap();

    run_bash(&repo, "tar", &["xzf", "pkg.tar.gz"]);

    let post = snapshot(&root, "tar-extract-sess", "tar-extract-t2", None).unwrap();
    let result = diff(&pre, &post);
    assert!(
        result
            .created
            .iter()
            .any(|p| p.display().to_string().contains("alpha.txt")),
        "alpha.txt should appear as created after tarball extract; got created={:?}",
        result.created,
    );
    assert!(
        result
            .created
            .iter()
            .any(|p| p.display().to_string().contains("beta.txt")),
        "beta.txt should appear as created after tarball extract; got created={:?}",
        result.created,
    );
}

// ===========================================================================
// Category 12: Compiler/tool output simulation
// ===========================================================================

#[test]
fn test_bash_provenance_simulated_compile() {
    let repo = TestRepo::new();
    let root = repo_root(&repo);
    add_and_commit(
        &repo,
        "hello.c",
        "#include <stdio.h>\nint main() { printf(\"hello\\n\"); return 0; }\n",
        "initial commit",
    );

    pre_hook(&root, "compile-sess", "compile-t1");

    // Simulate compilation by creating an output binary
    run_bash(&repo, "sh", &["-c", "echo 'compiled binary' > hello"]);

    let post_action = post_hook(&root, "compile-sess", "compile-t1");
    assert_checkpoint_contains(&post_action, "hello");
}
