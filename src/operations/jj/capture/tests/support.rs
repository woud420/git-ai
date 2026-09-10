use super::*;

pub(in crate::operations::jj::capture) struct Fixture {
    _temp: tempfile::TempDir,
    pub ancestor: PathBuf,
    pub root: PathBuf,
    pub repo: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        // Canonicalize the test-owned root once, outside capture, for macOS /var.
        let ancestor = temp.path().canonicalize().unwrap().join("ancestor");
        let root = ancestor.join("workspace");
        for directory in [
            ".git/objects",
            ".jj/working_copy",
            ".jj/repo/store",
            ".jj/repo/op_store/operations",
            ".jj/repo/op_store/views",
            ".jj/repo/op_heads/heads",
        ] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        for (path, bytes) in [
            (".git/HEAD", "ref: refs/heads/main\n"),
            (".jj/working_copy/type", "local"),
            (".jj/repo/store/type", "git"),
            (".jj/repo/store/git_target", "../../../.git"),
            (".jj/repo/op_store/type", "simple_op_store"),
            (".jj/repo/op_heads/type", "simple_op_heads_store"),
        ] {
            fs::write(root.join(path), bytes).unwrap();
        }
        let fixture = Self {
            _temp: temp,
            ancestor,
            repo: root.join(".jj/repo"),
            root,
        };
        fixture.write_pair(MERGE_ID, MERGE_HEX, RICH_ID, RICH_HEX);
        fixture.set_heads(&[MERGE_ID]);
        fixture.write_checkout("default");
        fixture
    }

    pub fn context(&self) -> WorkspaceContext {
        discover(&self.root).unwrap()
    }

    pub fn heads(&self) -> PathBuf {
        self.repo.join("op_heads/heads")
    }

    pub fn set_heads(&self, ids: &[&str]) {
        let entries: Vec<_> = fs::read_dir(self.heads()).unwrap().take(36).collect();
        assert!(entries.len() <= 35);
        for entry in entries {
            fs::remove_file(entry.unwrap().path()).unwrap();
        }
        for id in ids {
            fs::write(self.heads().join(id), []).unwrap();
        }
    }

    pub fn write_checkout(&self, workspace: &str) {
        let mut bytes = vec![0x12, 64];
        bytes.extend(unhex(MERGE_ID));
        assert!(workspace.len() < 128);
        bytes.extend([0x1a, workspace.len() as u8]);
        bytes.extend_from_slice(workspace.as_bytes());
        fs::write(self.root.join(".jj/working_copy/checkout"), bytes).unwrap();
    }

    pub fn write_pair(&self, operation_id: &str, op: &str, view_id: &str, view: &str) {
        fs::write(
            self.repo.join("op_store/operations").join(operation_id),
            unhex(op),
        )
        .unwrap();
        fs::write(self.repo.join("op_store/views").join(view_id), unhex(view)).unwrap();
    }

    pub fn with_shared_view_heads() -> Self {
        let fixture = Self::new();
        fixture.write_pair(LEFT_ID, LEFT_HEX, MINIMAL_ID, MINIMAL_HEX);
        fixture.write_pair(RIGHT_ID, RIGHT_HEX, MINIMAL_ID, MINIMAL_HEX);
        fixture.set_heads(&[LEFT_ID, RIGHT_ID]);
        fixture
    }
}

fn unhex(value: &str) -> Vec<u8> {
    assert!(value.len().is_multiple_of(2));
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).unwrap();
            u8::from_str_radix(digits, 16).unwrap()
        })
        .collect()
}

pub(super) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

pub(super) struct Hooks<F> {
    callback: F,
    pub phases: Vec<CapturePhase>,
    now: Instant,
    pub expire_at: Option<(CapturePhase, Instant)>,
}

impl<F: FnMut(CapturePhase)> Hooks<F> {
    pub fn new(callback: F) -> Self {
        Self {
            callback,
            phases: Vec::new(),
            now: Instant::now(),
            expire_at: None,
        }
    }
}

impl<F: FnMut(CapturePhase)> CaptureHooks for Hooks<F> {
    fn phase(&mut self, phase: CapturePhase) {
        self.phases.push(phase);
        (self.callback)(phase);
        if let Some((at, instant)) = self.expire_at
            && phase == at
        {
            self.now = instant;
        }
    }

    fn now(&mut self) -> Instant {
        self.now
    }
}

pub(super) fn no_hooks() -> Hooks<impl FnMut(CapturePhase)> {
    Hooks::new(|_| {})
}

pub(super) fn attempt(
    context: &WorkspaceContext,
    budget: &mut CaptureBudget,
    hooks: &mut impl CaptureHooks,
) -> Result<CapturedJjCurrentState, JjCaptureError> {
    let result = capture_with(context, budget, hooks);
    assert_eq!(budget.counters().live_directory_descriptors, 0);
    result
}

pub(super) fn raw_size(evidence: &JjOperationEvidence) -> usize {
    evidence.operation_bytes.len() + evidence.view_bytes.len()
}

pub(super) fn require_error(
    result: Result<CapturedJjCurrentState, JjCaptureError>,
) -> JjCaptureError {
    match result {
        Ok(_) => panic!("capture accepted an invalid bounded fixture"),
        Err(error) => error,
    }
}
