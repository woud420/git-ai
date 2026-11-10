//! Multi-row span walks (rebase, pull --rebase, cherry-pick): which HEAD rows
//! one command owns, where it stops, and where it leaves the cursor.
use super::tests_fixtures::*;
use super::*;
use crate::model::domain::FamilyKey;
use std::fs;

struct Repo {
    _temp: tempfile::TempDir,
    worktree: PathBuf,
    git_dir: PathBuf,
    family: FamilyKey,
}

fn repo() -> Repo {
    let temp = tempfile::tempdir().unwrap();
    let worktree = temp.path().join("repo");
    let git_dir = worktree.join(".git");
    fs::create_dir_all(git_dir.join("logs/refs/heads")).unwrap();
    let family = FamilyKey::new(git_dir.to_string_lossy().to_string());
    Repo {
        _temp: temp,
        worktree,
        git_dir,
        family,
    }
}

fn oid(index: usize) -> String {
    format!("{:040x}", index + 1)
}

/// Writes `(old, new, message)` rows and returns the end offset of each row.
fn write_reflog(git_dir: &Path, reference: &str, rows: &[(usize, usize, &str)]) -> Vec<u64> {
    let oids: Vec<_> = rows.iter().map(|row| (oid(row.0), oid(row.1))).collect();
    let entries: Vec<_> = oids
        .iter()
        .zip(rows)
        .map(|((old, new), row)| (old.as_str(), new.as_str(), row.2))
        .collect();
    append_reflog(git_dir, reference, &entries);
    let text = fs::read_to_string(git_dir.join("logs").join(reference)).unwrap();
    let mut end = 0;
    text.split_inclusive('\n')
        .map(|line| {
            end += line.len() as u64;
            end
        })
        .collect()
}

fn head_changes(transitions: &[(usize, usize)]) -> Vec<RefChange> {
    transitions
        .iter()
        .map(|(old, new)| ref_change("HEAD", &oid(*old), &oid(*new)))
        .collect()
}

#[test]
fn rebase_span_walks_every_pick_and_stops_at_the_next_rebase() {
    let repo = repo();
    let ends = write_reflog(
        &repo.git_dir,
        "HEAD",
        &[
            (0, 1, "rebase (start): checkout main"),
            (1, 2, "rebase (pick): One"),
            (2, 3, "rebase (pick): Two"),
            (8, 9, "rebase (pick): Row that does not chain"),
            (3, 4, "rebase (pick): Three"),
            (4, 5, "rebase (pick): Four"),
            (5, 5, "rebase (finish): returning to refs/heads/topic"),
            (5, 6, "rebase (start): checkout main"),
            (6, 7, "rebase (pick): Later rebase"),
        ],
    );
    write_reflog(
        &repo.git_dir,
        "refs/heads/topic",
        &[(0, 5, "rebase (finish): refs/heads/topic onto main")],
    );
    let state = family_state(&repo.family);
    let mut cursor = RefCursor::new(repo.family.clone());
    let rebase = || command_with_worktree(&repo.family, Some(repo.worktree.clone()), &["rebase", "main"]);

    let mut first = rebase();
    cursor.enrich_command(&mut first, &state).unwrap();

    let mut expected = head_changes(&[(0, 1), (1, 2), (2, 3), (3, 4), (4, 5)]);
    expected.push(ref_change("refs/heads/topic", &oid(0), &oid(5)));
    assert_eq!(first.ref_changes, expected);
    let key = head_key(&repo.git_dir);
    assert_eq!(cursor.offsets.get(&key), Some(&ends[6]));
    assert!(!cursor.consumed_offsets.contains_key(&key));

    let mut later = rebase();
    cursor.enrich_command(&mut later, &state).unwrap();

    assert_eq!(later.ref_changes, head_changes(&[(5, 6), (6, 7)]));
    assert_eq!(cursor.offsets.get(&key), Some(&ends[8]));
}

#[test]
fn pull_rebase_span_walks_every_pick_and_stops_at_the_next_pull() {
    let repo = repo();
    let ends = write_reflog(
        &repo.git_dir,
        "HEAD",
        &[
            (0, 1, "pull --rebase (start): checkout upstream"),
            (1, 2, "pull --rebase (pick): One"),
            (2, 3, "pull --rebase (pick): Two"),
            (8, 9, "pull --rebase (pick): Row that does not chain"),
            (3, 4, "pull --rebase (pick): Three"),
            (4, 4, "pull --rebase (finish): returning to refs/heads/main"),
            (4, 5, "pull --rebase (start): checkout upstream"),
            (5, 6, "pull --rebase (pick): Later pull"),
        ],
    );
    write_reflog(
        &repo.git_dir,
        "refs/heads/main",
        &[(0, 4, "pull --rebase (finish): refs/heads/main onto upstream")],
    );
    let state = family_state(&repo.family);
    let mut cursor = RefCursor::new(repo.family.clone());
    let pull = || command_with_worktree(&repo.family, Some(repo.worktree.clone()), &["pull", "--rebase"]);

    let mut first = pull();
    cursor.enrich_command(&mut first, &state).unwrap();

    let mut expected = head_changes(&[(0, 1), (1, 2), (2, 3), (3, 4)]);
    expected.push(ref_change("refs/heads/main", &oid(0), &oid(4)));
    assert_eq!(first.ref_changes, expected);
    let key = head_key(&repo.git_dir);
    assert_eq!(cursor.offsets.get(&key), Some(&ends[5]));
    assert!(!cursor.consumed_offsets.contains_key(&key));

    let mut later = pull();
    cursor.enrich_command(&mut later, &state).unwrap();

    assert_eq!(later.ref_changes, head_changes(&[(4, 5), (5, 6)]));
    assert_eq!(cursor.offsets.get(&key), Some(&ends[7]));
}

#[test]
fn cherry_pick_span_stops_at_its_source_count_and_compacts_the_consumed_prefix() {
    let repo = repo();
    let ends = write_reflog(
        &repo.git_dir,
        "HEAD",
        &[
            (0, 1, "checkout: moving from main to topic"),
            (1, 2, "cherry-pick: One"),
            (2, 3, "commit (cherry-pick): Two"),
            (8, 9, "cherry-pick: Row that does not chain"),
            (3, 4, "cherry-pick: Three"),
            (4, 5, "cherry-pick: Four"),
            (5, 6, "cherry-pick: Five"),
        ],
    );
    let state = family_state(&repo.family);
    let mut cursor = RefCursor::new(repo.family.clone());
    let key = head_key(&repo.git_dir);
    cursor.offsets.insert(key.clone(), ends[0]);
    cursor.pending_cherry_pick_source_oids = (10..14).map(oid).collect();
    let mut cmd = command_with_worktree(
        &repo.family,
        Some(repo.worktree.clone()),
        &["cherry-pick", "--continue"],
    );

    cursor.enrich_command(&mut cmd, &state).unwrap();

    assert_eq!(
        cmd.ref_changes,
        head_changes(&[(1, 2), (2, 3), (3, 4), (4, 5)])
    );
    // The cursor advances over the consumed prefix only; rows consumed past
    // the unowned row stay marked until the cursor reaches them.
    assert_eq!(cursor.offsets.get(&key), Some(&ends[2]));
    assert_eq!(
        cursor.consumed_offsets.get(&key),
        Some(&HashSet::from([ends[4], ends[5]]))
    );
}
