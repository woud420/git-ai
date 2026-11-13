use super::*;
use tempfile::TempDir;

fn attr(author: &str) -> Vec<LineAttribution> {
    vec![LineAttribution::new(1, 1, author.to_string(), None)]
}

/// Regression (#9): merge_working_log_dirs (via rename_working_log when the
/// destination already exists) must preserve OLD-base INITIAL entries on a
/// shared key, per the documented "preserve the old-base entries first".
/// The old code used HashMap::extend(new), so `new` clobbered `old` for any
/// shared path. Each side's unique entries must also survive.
#[test]
fn test_merge_working_log_dirs_old_base_wins_on_conflict() {
    let tmp = TempDir::new().unwrap();
    let workdir = tmp.path().join("workdir");
    fs::create_dir_all(&workdir).unwrap();
    let ai_dir = tmp.path().join("ai");
    let storage = RepoStorage::for_repo_path(&ai_dir, &workdir).unwrap();

    let old_sha = "1111111111111111111111111111111111111111";
    let new_sha = "2222222222222222222222222222222222222222";

    // OLD base: shared.txt -> old author, plus a unique old-only file.
    let old_log = storage.working_log_for_base_commit(old_sha).unwrap();
    let mut old_initial = InitialAttributions::default();
    old_initial.files.insert("shared.txt".into(), attr("h_OLD"));
    old_initial
        .files
        .insert("old_only.txt".into(), attr("h_OLD"));
    old_initial
        .file_blobs
        .insert("shared.txt".into(), "OLD CONTENT".into());
    old_log.write_initial(old_initial).unwrap();

    // NEW base: shared.txt -> new author (conflict), plus a unique new-only file.
    let new_log = storage.working_log_for_base_commit(new_sha).unwrap();
    let mut new_initial = InitialAttributions::default();
    new_initial
        .files
        .insert("shared.txt".into(), attr("ai_NEW"));
    new_initial
        .files
        .insert("new_only.txt".into(), attr("ai_NEW"));
    new_initial
        .file_blobs
        .insert("shared.txt".into(), "NEW CONTENT".into());
    new_log.write_initial(new_initial).unwrap();

    // Merge old into new (destination already exists).
    storage.rename_working_log(old_sha, new_sha).unwrap();

    let merged = storage
        .working_log_for_base_commit(new_sha)
        .unwrap()
        .read_initial_attributions();

    // Shared key: OLD base wins.
    assert_eq!(
        merged
            .files
            .get("shared.txt")
            .map(|a| a[0].author_id.as_str()),
        Some("h_OLD"),
        "old-base attribution must win on a shared path"
    );
    assert_eq!(
        merged.file_blobs.get("shared.txt").map(|s| s.as_str()),
        Some("OLD CONTENT"),
        "old-base blob must win on a shared path (kept consistent with files)"
    );
    // Both sides' unique entries survive.
    assert!(merged.files.contains_key("old_only.txt"));
    assert!(merged.files.contains_key("new_only.txt"));
}
