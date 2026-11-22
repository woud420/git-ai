use git_ai::model::checkpoint_delivery::{CHECKPOINT_DELIVERY_SCHEMA_VERSION, CheckpointDelivery};
use git_ai::model::checkpoint_request::{
    BaseCommit, CheckpointFile, CheckpointRequest, PreparedPathRole,
};
use git_ai::model::working_log::{AgentId, CheckpointKind};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) struct Fixture {
    pub(super) name: &'static str,
    pub(super) files: usize,
    bytes_per_file: usize,
    pattern: &'static str,
}

pub(super) const FIXTURES: [Fixture; 4] = [
    Fixture {
        name: "small",
        files: 1,
        bytes_per_file: 1024,
        pattern: "let value = compute_value();\n",
    },
    Fixture {
        name: "large",
        files: 1,
        bytes_per_file: 1024 * 1024,
        pattern: "let value = compute_value();\n",
    },
    Fixture {
        name: "many_files",
        files: 64,
        bytes_per_file: 4 * 1024,
        pattern: "let value = compute_value();\n",
    },
    Fixture {
        name: "escaping_utf8",
        files: 1,
        bytes_per_file: 64 * 1024,
        pattern: "\"quoted\" \\path\\to\\file\t日本語 café 🦀\r\n",
    },
];

impl Fixture {
    pub(super) fn delivery(self, root: &Path, iteration: usize) -> CheckpointDelivery {
        let files = (0..self.files)
            .map(|index| {
                let mut content = format!("sample {iteration:08} file {index:08}\n");
                let repetitions = self.bytes_per_file.div_ceil(self.pattern.len());
                content.push_str(&self.pattern.repeat(repetitions));
                let mut end = self.bytes_per_file;
                while !content.is_char_boundary(end) {
                    end -= 1;
                }
                content.truncate(end);
                CheckpointFile {
                    path: root.join(format!("payload-{index:03}.txt")),
                    content: Some(content),
                    repo_work_dir: root.to_path_buf(),
                    base_commit: BaseCommit::Initial,
                }
            })
            .collect();
        CheckpointDelivery {
            schema_version: CHECKPOINT_DELIVERY_SCHEMA_VERSION,
            delivery_id: format!("ipc-{}-{iteration:08}", self.name),
            batch_id: format!("ipc-{}-{iteration:08}", self.name),
            batch_ordinal: 0,
            captured_at_unix_ms: 1_788_375_035_000,
            producer_version: env!("CARGO_PKG_VERSION").to_owned(),
            request: CheckpointRequest {
                trace_id: format!("ipc-{}-{iteration:08}", self.name),
                checkpoint_kind: CheckpointKind::AiAgent,
                agent_id: Some(AgentId {
                    tool: "mock_ai".to_owned(),
                    id: "ipc-benchmark-session".to_owned(),
                    model: "deterministic-fixture".to_owned(),
                }),
                files,
                path_role: PreparedPathRole::Edited,
                stream_source: None,
                metadata: HashMap::from([
                    ("fixture".to_owned(), self.name.to_owned()),
                    ("escaped".to_owned(), "\"quoted\" \\ café 🦀".to_owned()),
                ]),
                delivery_id: None,
            },
        }
    }

    pub(super) fn content_bytes(self) -> usize {
        self.delivery(&std::env::temp_dir().join("ipc-fixture"), 7)
            .request
            .files
            .iter()
            .map(|file| file.content.as_ref().unwrap().len())
            .sum()
    }
}
