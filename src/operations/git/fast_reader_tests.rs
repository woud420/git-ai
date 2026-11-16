use super::*;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;
use tempfile::TempDir;

fn setup_git_dir() -> TempDir {
    let temp = TempDir::new().unwrap();
    fs::create_dir_all(temp.path().join("refs/heads")).unwrap();
    fs::create_dir_all(temp.path().join("objects")).unwrap();
    temp
}

fn write_loose_object(git_dir: &Path, sha: &str, obj_type: &str, content: &[u8]) {
    let obj_dir = git_dir.join("objects").join(&sha[..2]);
    fs::create_dir_all(&obj_dir).unwrap();

    let header = format!("{} {}\0", obj_type, content.len());
    let mut full_content = header.as_bytes().to_vec();
    full_content.extend_from_slice(content);

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&full_content).unwrap();
    let compressed = encoder.finish().unwrap();

    let obj_path = obj_dir.join(&sha[2..]);
    fs::write(obj_path, compressed).unwrap();
}

// ===== FastRefReader tests =====

#[test]
fn test_read_head_symbolic_ref() {
    let temp = setup_git_dir();
    fs::write(temp.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_read_head();
    assert_eq!(
        result,
        Some(HeadKind::Symbolic("refs/heads/main".to_string()))
    );
}

#[test]
fn test_read_head_detached() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    fs::write(temp.path().join("HEAD"), format!("{}\n", sha)).unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_read_head();
    assert_eq!(result, Some(HeadKind::Detached(sha.to_string())));
}

#[test]
fn test_read_head_invalid_format_returns_none() {
    let temp = setup_git_dir();
    fs::write(temp.path().join("HEAD"), "garbage content\n").unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    assert_eq!(reader.try_read_head(), None);
}

#[test]
fn test_read_head_missing_returns_none() {
    let temp = TempDir::new().unwrap();
    let reader = FastRefReader::new(temp.path(), temp.path());
    assert_eq!(reader.try_read_head(), None);
}

#[test]
fn test_resolve_loose_ref() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    fs::write(temp.path().join("refs/heads/main"), format!("{}\n", sha)).unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_resolve_ref("refs/heads/main");
    assert_eq!(result, Some(sha.to_string()));
}

#[test]
fn test_resolve_packed_ref() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    let packed_content = format!(
        "# pack-refs with: peeled fully-peeled sorted\n{} refs/heads/packed-branch\n",
        sha
    );
    fs::write(temp.path().join("packed-refs"), packed_content).unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_resolve_ref("refs/heads/packed-branch");
    assert_eq!(result, Some(sha.to_string()));
}

#[test]
fn test_resolve_ref_not_found() {
    let temp = setup_git_dir();
    let reader = FastRefReader::new(temp.path(), temp.path());
    assert_eq!(reader.try_resolve_ref("refs/heads/nonexistent"), None);
}

#[test]
fn test_resolve_symbolic_ref_indirection() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    fs::create_dir_all(temp.path().join("refs/remotes/origin")).unwrap();
    fs::write(
        temp.path().join("refs/remotes/origin/HEAD"),
        "ref: refs/remotes/origin/main\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("refs/remotes/origin/main"),
        format!("{}\n", sha),
    )
    .unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_resolve_ref("refs/remotes/origin/HEAD");
    assert_eq!(result, Some(sha.to_string()));
}

#[test]
fn test_resolve_head_resolves_through_symbolic() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    fs::write(temp.path().join("HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(temp.path().join("refs/heads/main"), format!("{}\n", sha)).unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_resolve_ref("HEAD");
    assert_eq!(result, Some(sha.to_string()));
}

#[test]
fn test_resolve_ref_worktree_common_dir_priority() {
    // Simulate a linked worktree: refs live in common_dir, not git_dir
    let common = setup_git_dir();
    let worktree_git_dir = TempDir::new().unwrap();
    fs::create_dir_all(worktree_git_dir.path()).unwrap();

    let sha = "abc123def456789012345678901234567890abcd";
    fs::write(common.path().join("refs/heads/main"), format!("{}\n", sha)).unwrap();

    let reader = FastRefReader::new(worktree_git_dir.path(), common.path());
    let result = reader.try_resolve_ref("refs/heads/main");
    assert_eq!(result, Some(sha.to_string()));
}

#[test]
fn test_resolve_ref_loose_in_git_dir_over_packed() {
    let temp = setup_git_dir();
    let loose_sha = "1111111111111111111111111111111111111111";
    let packed_sha = "2222222222222222222222222222222222222222";

    fs::write(
        temp.path().join("refs/heads/main"),
        format!("{}\n", loose_sha),
    )
    .unwrap();
    let packed_content = format!("# pack-refs with: peeled\n{} refs/heads/main\n", packed_sha);
    fs::write(temp.path().join("packed-refs"), packed_content).unwrap();

    let reader = FastRefReader::new(temp.path(), temp.path());
    let result = reader.try_resolve_ref("refs/heads/main");
    assert_eq!(result, Some(loose_sha.to_string()));
}

// ===== FastObjectReader tests =====

#[test]
fn test_read_loose_blob() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    let content = b"Hello, World!";
    write_loose_object(temp.path(), sha, "blob", content);

    let reader = FastObjectReader::new(temp.path());
    let result = reader.try_read_blob(sha);
    assert_eq!(result, Some(content.to_vec()));
}

#[test]
fn test_read_nonexistent_blob() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";

    let reader = FastObjectReader::new(temp.path());
    assert_eq!(reader.try_read_blob(sha), None);
}

#[test]
fn test_read_commit_as_blob_returns_none() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    let content = b"tree def456789012345678901234567890abcdef01\nauthor Test <test@example.com>";
    write_loose_object(temp.path(), sha, "commit", content);

    let reader = FastObjectReader::new(temp.path());
    assert_eq!(reader.try_read_blob(sha), None);
}

#[test]
fn test_read_object_type() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    write_loose_object(temp.path(), sha, "blob", b"content");

    let reader = FastObjectReader::new(temp.path());
    assert_eq!(reader.try_read_object_type(sha), Some("blob".to_string()));
}

#[test]
fn test_read_commit_tree_oid() {
    let temp = setup_git_dir();
    let commit_sha = "abc123def456789012345678901234567890abcd";
    let tree_sha = "def456789012345678901234567890abcdef0123";
    let commit_body = format!(
        "tree {}\nparent 0000000000000000000000000000000000000000\nauthor A <a@b.c> 1 +0000\ncommitter A <a@b.c> 1 +0000\n\nmessage\n",
        tree_sha
    );
    write_loose_object(temp.path(), commit_sha, "commit", commit_body.as_bytes());

    let reader = FastObjectReader::new(temp.path());
    let result = reader.try_read_commit_tree_oid(commit_sha);
    assert_eq!(result, Some(tree_sha.to_string()));
}

#[test]
fn test_tree_entry_for_path_single_level() {
    let temp = setup_git_dir();
    let tree_sha = "abc123def456789012345678901234567890abcd";
    let blob_sha_bytes: [u8; 20] = [
        0xde, 0xf4, 0x56, 0x78, 0x90, 0x12, 0x34, 0x56, 0x78, 0x90, 0x12, 0x34, 0x56, 0x78, 0x90,
        0xab, 0xcd, 0xef, 0x01, 0x23,
    ];
    let expected_blob_oid = "def456789012345678901234567890abcdef0123";

    // Build tree content: "100644 file.txt\0<20-byte-sha>"
    let mut tree_content = Vec::new();
    tree_content.extend_from_slice(b"100644 file.txt\0");
    tree_content.extend_from_slice(&blob_sha_bytes);

    write_loose_object(temp.path(), tree_sha, "tree", &tree_content);

    let reader = FastObjectReader::new(temp.path());
    let result = reader.try_tree_entry_for_path(tree_sha, Path::new("file.txt"));
    assert_eq!(result, Some(expected_blob_oid.to_string()));
}

#[test]
fn test_tree_entry_for_path_nested() {
    let temp = setup_git_dir();

    // Create blob
    let blob_sha_bytes: [u8; 20] = [
        0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88,
        0x99, 0xaa, 0xbb, 0xcc, 0xdd,
    ];
    let expected_blob_oid = "aabbccddeeff00112233445566778899aabbccdd";

    // Create subtree containing the blob
    let subtree_sha = "1111111111111111111111111111111111111111";
    let subtree_sha_bytes: [u8; 20] = [
        0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11, 0x11, 0x11,
    ];

    let mut subtree_content = Vec::new();
    subtree_content.extend_from_slice(b"100644 main.rs\0");
    subtree_content.extend_from_slice(&blob_sha_bytes);
    write_loose_object(temp.path(), subtree_sha, "tree", &subtree_content);

    // Create root tree containing the subtree
    let root_tree_sha = "2222222222222222222222222222222222222222";
    let mut root_content = Vec::new();
    root_content.extend_from_slice(b"40000 src\0");
    root_content.extend_from_slice(&subtree_sha_bytes);
    write_loose_object(temp.path(), root_tree_sha, "tree", &root_content);

    let reader = FastObjectReader::new(temp.path());
    let result = reader.try_tree_entry_for_path(root_tree_sha, Path::new("src/main.rs"));
    assert_eq!(result, Some(expected_blob_oid.to_string()));
}

#[test]
fn test_tree_entry_for_path_not_found() {
    let temp = setup_git_dir();
    let tree_sha = "abc123def456789012345678901234567890abcd";
    let blob_sha_bytes: [u8; 20] = [0xde; 20];

    let mut tree_content = Vec::new();
    tree_content.extend_from_slice(b"100644 other.txt\0");
    tree_content.extend_from_slice(&blob_sha_bytes);
    write_loose_object(temp.path(), tree_sha, "tree", &tree_content);

    let reader = FastObjectReader::new(temp.path());
    let result = reader.try_tree_entry_for_path(tree_sha, Path::new("missing.txt"));
    assert_eq!(result, None);
}

#[test]
fn test_alternates_causes_fallback() {
    let temp = setup_git_dir();
    let sha = "abc123def456789012345678901234567890abcd";
    write_loose_object(temp.path(), sha, "blob", b"content");

    // Create alternates file
    fs::create_dir_all(temp.path().join("objects/info")).unwrap();
    fs::write(
        temp.path().join("objects/info/alternates"),
        "/some/other/objects\n",
    )
    .unwrap();

    let reader = FastObjectReader::new(temp.path());
    assert_eq!(reader.try_read_blob(sha), None);
}

#[test]
fn test_invalid_oid_returns_none() {
    let temp = setup_git_dir();
    let reader = FastObjectReader::new(temp.path());
    assert_eq!(reader.try_read_blob("not-a-valid-oid"), None);
    assert_eq!(reader.try_read_blob(""), None);
    assert_eq!(reader.try_read_blob("abc"), None);
}

#[test]
fn test_read_commit_parents_in_order() {
    let temp = setup_git_dir();
    let tree = "def456789012345678901234567890abcdef0123";
    let (first, second) = (
        "1111111111111111111111111111111111111111",
        "2222222222222222222222222222222222222222",
    );
    let commits = [
        ("abc123def456789012345678901234567890abcd", vec![]),
        ("bcd123def456789012345678901234567890abcd", vec![first]),
        (
            "cde123def456789012345678901234567890abcd",
            vec![first, second],
        ),
    ];
    for (sha, parents) in &commits {
        let parent_lines: String = parents.iter().map(|p| format!("parent {p}\n")).collect();
        let body = format!("tree {tree}\n{parent_lines}author A <a@b.c> 1 +0000\n\nparent x\n");
        write_loose_object(temp.path(), sha, "commit", body.as_bytes());
    }

    let reader = FastObjectReader::new(temp.path());
    for (sha, parents) in &commits {
        let expected: Vec<String> = parents.iter().map(|p| p.to_string()).collect();
        assert_eq!(reader.try_read_commit_parents(sha), Some(expected));
    }
    assert_eq!(reader.try_read_commit_parents(tree), None);
}

#[test]
fn test_read_commit_parents_falls_back_when_parents_may_be_rewritten() {
    let sha = "abc123def456789012345678901234567890abcd";
    let body = "tree def456789012345678901234567890abcdef0123\nparent 1111111111111111111111111111111111111111\n\nm\n";
    let replace_ref = format!("refs/replace/{sha}");
    let setups: [&dyn Fn(&Path); 4] = [
        &|dir| fs::write(dir.join("shallow"), "x\n").unwrap(),
        &|dir| {
            fs::create_dir_all(dir.join("info")).unwrap();
            fs::write(dir.join("info/grafts"), "x\n").unwrap();
        },
        &|dir| {
            fs::create_dir_all(dir.join("refs/replace")).unwrap();
            fs::write(dir.join(&replace_ref), format!("{sha}\n")).unwrap();
        },
        &|dir| fs::write(dir.join("packed-refs"), format!("{sha} {replace_ref}\n")).unwrap(),
    ];
    for setup in setups {
        let temp = setup_git_dir();
        write_loose_object(temp.path(), sha, "commit", body.as_bytes());
        let reader = FastObjectReader::new(temp.path());
        assert!(reader.try_read_commit_parents(sha).is_some());
        setup(temp.path());
        assert_eq!(reader.try_read_commit_parents(sha), None);
    }
}

#[test]
fn commit_parents_match_git_show_with_and_without_replace_refs() {
    let tmp = crate::operations::git::test_utils::TmpRepo::new().unwrap();
    let git = |args: &[&str]| tmp.git_command(args).unwrap().trim().to_string();
    let root = tmp.commit_all("root").unwrap();
    git(&["checkout", "-q", "-b", "side"]);
    let side = tmp.commit_all("side").unwrap();
    git(&["checkout", "-q", "-"]);
    let main = tmp.commit_all("main").unwrap();
    git(&["merge", "-q", "--no-ff", "-m", "merge", "side"]);
    let merge = git(&["rev-parse", "HEAD"]);

    let check = |sha: &str| {
        let commit = tmp.gitai_repo().find_commit(sha.to_string()).unwrap();
        let fast: Vec<String> = commit.parents().map(|parent| parent.id()).collect();
        let shown = git(&["show", "-s", "--format=%P", sha]);
        assert_eq!(fast.join(" "), shown, "parents of {sha}");
    };
    for sha in [&root, &side, &main, &merge] {
        check(sha);
    }
    let reader = FastObjectReader::new(tmp.gitai_repo().common_dir());
    assert!(reader.try_read_commit_parents(&merge).is_some());

    git(&["replace", "--graft", &merge, &main]);
    assert_eq!(reader.try_read_commit_parents(&merge), None);
    check(&merge);
}
