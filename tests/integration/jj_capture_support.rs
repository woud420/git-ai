use super::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Fixture {
    pub repo: TestRepo,
    pub root: PathBuf,
    pub repo_dir: PathBuf,
}

impl Fixture {
    pub fn new(colocated: bool) -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let root = if colocated {
            repo.path().to_owned()
        } else {
            repo.path().join("primary workspace")
        };
        let repo_dir = layout(&root, colocated);
        let fixture = Self {
            repo,
            root,
            repo_dir,
        };
        fixture.write_evidence(&merge());
        fixture.set_heads(&[MERGE_ID]);
        fixture.write_checkout(MERGE_ID, "default");
        fs::write(fixture.root.join("dirty.txt"), "never snapshotted\n").unwrap();
        fixture
    }

    pub fn context(&self) -> WorkspaceContext {
        discover(&self.root).unwrap()
    }

    pub fn heads_dir(&self) -> PathBuf {
        self.repo_dir.join("op_heads/heads")
    }

    pub fn operation_path(&self, id: &str) -> PathBuf {
        self.repo_dir.join("op_store/operations").join(id)
    }

    pub fn view_path(&self, id: &str) -> PathBuf {
        self.repo_dir.join("op_store/views").join(id)
    }

    pub fn checkout_path(&self) -> PathBuf {
        self.root.join(".jj/working_copy/checkout")
    }

    pub fn write_evidence(&self, evidence: &JjOperationEvidence) {
        write_evidence_to(&self.repo_dir, evidence);
    }

    pub fn set_heads(&self, ids: &[&str]) {
        let entries: Vec<_> = fs::read_dir(self.heads_dir()).unwrap().take(36).collect();
        assert!(entries.len() <= 35);
        for entry in entries {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        for id in ids {
            fs::write(self.heads_dir().join(id), []).unwrap();
        }
    }

    pub fn write_checkout(&self, id: &str, workspace: &str) {
        fs::write(self.checkout_path(), checkout_bytes(id, workspace)).unwrap();
    }
}

pub fn write_evidence_to(repo_dir: &Path, evidence: &JjOperationEvidence) {
    fs::write(
        repo_dir
            .join("op_store/operations")
            .join(&evidence.operation_id),
        &evidence.operation_bytes,
    )
    .unwrap();
    fs::write(
        repo_dir.join("op_store/views").join(&evidence.view_id),
        &evidence.view_bytes,
    )
    .unwrap();
}

pub fn layout(root: &Path, colocated: bool) -> PathBuf {
    let repo_dir = root.join(".jj/repo");
    for directory in [
        ".jj/working_copy",
        ".jj/repo/store",
        ".jj/repo/op_store/operations",
        ".jj/repo/op_store/views",
        ".jj/repo/op_heads/heads",
    ] {
        fs::create_dir_all(root.join(directory)).unwrap();
    }
    for (path, bytes) in [
        (".jj/working_copy/type", "local"),
        (".jj/repo/store/type", "git"),
        (".jj/repo/op_store/type", "simple_op_store"),
        (".jj/repo/op_heads/type", "simple_op_heads_store"),
    ] {
        fs::write(root.join(path), bytes).unwrap();
    }
    let target = if colocated {
        "../../../.git"
    } else {
        let git_dir = repo_dir.join("store/git");
        minimal_git(&git_dir);
        "git"
    };
    fs::write(repo_dir.join("store/git_target"), target).unwrap();
    repo_dir
}

pub fn minimal_git(path: &Path) {
    fs::create_dir_all(path.join("objects")).unwrap();
    fs::write(path.join("HEAD"), "ref: refs/heads/main\n").unwrap();
}

pub fn checkout_bytes(id: &str, workspace: &str) -> Vec<u8> {
    [
        bytes_field(2, &unhex(id)),
        bytes_field(3, workspace.as_bytes()),
    ]
    .concat()
}

pub fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Entry {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

// Unlike diagnostic snapshot(), this fixture manifest never follows a symlink.
pub fn manifest(root: &Path) -> BTreeMap<PathBuf, Entry> {
    manifest_excluding(root, &[])
}

pub fn manifest_excluding(root: &Path, excluded: &[&str]) -> BTreeMap<PathBuf, Entry> {
    fn visit(root: &Path, path: &Path, excluded: &[&str], result: &mut BTreeMap<PathBuf, Entry>) {
        let entries: Vec<_> = fs::read_dir(path).unwrap().take(1025).collect();
        assert!(entries.len() <= 1024);
        for entry in entries {
            assert!(result.len() < 2048);
            let entry = entry.unwrap();
            let path = entry.path();
            let key = path.strip_prefix(root).unwrap().to_owned();
            // Opening and closing an unrelated descriptor can release SQLite's
            // process-wide POSIX locks; discarding its bytes afterward is too late.
            if excluded.iter().any(|name| key == Path::new(name)) {
                // Exclude this entry only; descendants of an unexpected directory
                // were included by the old post-traversal removal too.
                if entry.file_type().unwrap().is_dir() {
                    visit(root, &path, excluded, result);
                }
                continue;
            }
            let kind = entry.file_type().unwrap();
            let value = if kind.is_symlink() {
                Entry::Symlink(fs::read_link(&path).unwrap())
            } else if kind.is_dir() {
                Entry::Directory
            } else {
                assert!(kind.is_file());
                assert!(entry.metadata().unwrap().len() <= 2 * 1024 * 1024);
                Entry::File(fs::read(&path).unwrap())
            };
            result.insert(key, value);
            if kind.is_dir() {
                visit(root, &path, excluded, result);
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, excluded, &mut result);
    result
}

pub fn checked_capture(
    fixture: &Fixture,
    context: &WorkspaceContext,
    deadline: Instant,
) -> Result<CapturedJjCurrentState, JjCaptureError> {
    let before = manifest(fixture.repo.path());
    let home_before = manifest(fixture.repo.test_home_path());
    let result = capture_current_state(context, deadline);
    assert_eq!(manifest(fixture.repo.path()), before);
    assert_eq!(manifest(fixture.repo.test_home_path()), home_before);
    result
}

pub fn rejected(fixture: &Fixture, context: &WorkspaceContext, category: Option<&str>) {
    let error = match checked_capture(fixture, context, deadline()) {
        Ok(_) => panic!("invalid fixture produced a capture"),
        Err(error) => error,
    };
    let standard: &dyn std::error::Error = &error;
    assert!(!standard.to_string().is_empty());
    if let Some(category) = category {
        assert!(
            standard.to_string().to_ascii_lowercase().contains(category),
            "{standard}"
        );
    }
}
