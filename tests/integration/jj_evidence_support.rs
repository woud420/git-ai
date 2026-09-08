use super::*;
use std::path::PathBuf;

pub fn paired(
    operation_id: &str,
    operation_hex: &str,
    parents: &[&str],
    view_id: &str,
    view_hex: &str,
) -> JjOperationEvidence {
    JjOperationEvidence {
        operation_id: operation_id.to_owned(),
        parent_ids: parents.iter().map(|id| (*id).to_owned()).collect(),
        view_id: view_id.to_owned(),
        operation_bytes: unhex(operation_hex),
        view_bytes: unhex(view_hex),
    }
}

pub fn first() -> JjOperationEvidence {
    paired(
        FIRST_ID,
        FIRST_HEX,
        &[&"00".repeat(64)],
        MINIMAL_ID,
        MINIMAL_HEX,
    )
}

pub fn left() -> JjOperationEvidence {
    paired(LEFT_ID, LEFT_HEX, &[FIRST_ID], MINIMAL_ID, MINIMAL_HEX)
}

pub fn right() -> JjOperationEvidence {
    paired(RIGHT_ID, RIGHT_HEX, &[FIRST_ID], MINIMAL_ID, MINIMAL_HEX)
}

pub fn merge() -> JjOperationEvidence {
    paired(MERGE_ID, MERGE_HEX, &[LEFT_ID, RIGHT_ID], RICH_ID, RICH_HEX)
}

pub struct Fixture {
    pub repo: TestRepo,
    pub path: PathBuf,
    pub source: String,
}

impl Fixture {
    pub fn new() -> Self {
        let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
        let path = repo.test_home_path().join("jj-evidence.sqlite");
        Self {
            repo,
            path,
            source: "01".repeat(32),
        }
    }

    pub fn open(&self) -> JjObservationJournal {
        JjObservationJournal::open_at_path(&self.path).unwrap()
    }

    pub fn batch(&self, head: &str, operations: Vec<JjOperationEvidence>) -> JjObservationBatch {
        JjObservationBatch {
            schema_version: JJ_OBSERVATION_SCHEMA_VERSION,
            reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
            source_id: self.source.clone(),
            expected_generation: 0,
            expected_observed_heads: vec![],
            captured_integrated_heads: vec![head.to_owned()],
            operations,
        }
    }
}
