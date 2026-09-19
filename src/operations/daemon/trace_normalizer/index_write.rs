use super::PendingTraceCommand;
use crate::model::domain::IndexWriteEvidence;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct IndexVersions {
    read: bool,
    written: bool,
    unsupported: bool,
}

impl IndexVersions {
    pub fn proven_v2(&self) -> bool {
        self.read && self.written && !self.unsupported
    }
}

pub(super) fn record(pending: &mut PendingTraceCommand, payload: &Value, sid: &str, root: &str) {
    if sid != root || !matches!(pending.root_cmd_name.as_deref(), Some("reset" | "switch" | "checkout")) {
        return;
    }
    let category = payload.get("category").and_then(Value::as_str);
    let key = payload.get("key").and_then(Value::as_str);
    let label = payload.get("label").and_then(Value::as_str);
    // Git emits some monitor receipts without a repository id. These and
    // shared-index receipts disqualify the whole root, even if v2 follows.
    if matches!(category, Some("fsmonitor" | "fsm_hook" | "fsm_client"))
        || category == Some("index")
            && (key.is_some_and(|key| key.starts_with("extension/fsmn/"))
                || label.is_some_and(|label| label.starts_with("shared/")))
    {
        pending.index_versions.unsupported = true;
    }
    if payload.get("repo").and_then(Value::as_u64) != Some(1) || category != Some("index") {
        return;
    }
    if payload.get("event").and_then(Value::as_str) == Some("data") {
        match key {
            Some("read/version") => pending.index_versions.read = true,
            Some("write/version") => pending.index_versions.written = true,
            _ => return,
        }
        pending.index_versions.unsupported |=
            payload.get("value").and_then(Value::as_str) != Some("2");
        return;
    }
    if payload.get("label").and_then(Value::as_str) != Some("do_write_index") {
        return;
    }
    let Some(path) = payload.get("msg").and_then(Value::as_str).filter(|path| {
        path.len() <= 4096
            && !path.contains(['\0', '\n', '\r', '\u{fffd}'])
            && Path::new(path).is_absolute()
    }) else {
        pending.index_write = IndexWriteEvidence::Conflicting;
        return;
    };
    match &pending.index_write {
        IndexWriteEvidence::Missing => pending.index_write = IndexWriteEvidence::Exact(path.into()),
        IndexWriteEvidence::Exact(previous) if previous != Path::new(path) => {
            pending.index_write = IndexWriteEvidence::Conflicting;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::domain::NormalizedCommand;
    use crate::operations::daemon::trace_normalizer::TraceNormalizer;
    use crate::operations::daemon::trace_normalizer::tests_lifecycle::{
        MockBackend, atexit_payload,
    };
    use serde_json::json;
    use std::sync::Arc;

    fn normalized(frames: &[Value]) -> NormalizedCommand {
        let mut normalizer = TraceNormalizer::new(Arc::new(MockBackend::default()));
        normalizer
            .ingest_payload(
                &json!({"event":"start","sid":"root","ts":1,"argv":["git","reset","--hard"]}),
            )
            .unwrap();
        normalizer
            .ingest_payload(&json!({"event":"cmd_name","sid":"root","ts":2,"name":"reset"}))
            .unwrap();
        for frame in frames {
            normalizer.ingest_payload(frame).unwrap();
        }
        normalizer
            .ingest_payload(&atexit_payload("root", 4))
            .unwrap()
            .unwrap()
    }

    fn frames() -> Vec<Value> {
        vec![
            json!({"event":"data","sid":"root","ts":3,"repo":1,"category":"index","key":"read/version","value":"2"}),
            json!({"event":"region_enter","sid":"root","ts":3,"repo":1,"category":"index","label":"do_write_index","msg":std::env::temp_dir().join("index.lock")}),
            json!({"event":"data","sid":"root","ts":3,"repo":1,"category":"index","key":"write/version","value":"2"}),
        ]
    }

    #[test]
    fn reset_index_context_requires_root_owned_complete_v2_receipts() {
        let input = frames();
        let command = normalized(&input);
        assert!(command.index_v2);
        assert!(matches!(command.index_write, IndexWriteEvidence::Exact(_)));
        for omitted in [0, 2] {
            let mut incomplete = input.clone();
            incomplete.remove(omitted);
            assert!(!normalized(&incomplete).index_v2);
        }
        for property in ["sid", "repo"] {
            let mut child = input.clone();
            for frame in &mut child {
                frame[property] = if property == "sid" {
                    json!("root/child")
                } else {
                    json!(2)
                };
            }
            let ignored = normalized(&child);
            assert!(!ignored.index_v2);
            assert_eq!(ignored.index_write, IndexWriteEvidence::Missing);
        }
        let mut old = serde_json::to_value(command).unwrap();
        old.as_object_mut().unwrap().remove("index_v2");
        old.as_object_mut().unwrap().remove("index_write");
        let old: NormalizedCommand = serde_json::from_value(old).unwrap();
        assert!(!old.index_v2);
        assert_eq!(old.index_write, IndexWriteEvidence::Missing);
    }

    #[test]
    fn reset_index_context_keeps_conflicting_and_extended_evidence_unsupported() {
        for value in [json!("3"), json!("4"), Value::Null] {
            let mut input = frames();
            input[0]["value"] = value;
            input.push(frames()[0].clone());
            assert!(!normalized(&input).index_v2);
        }
        for category in ["fsmonitor", "fsm_hook", "fsm_client"] {
            let mut input = frames();
            input.push(json!({"event":"region_enter","sid":"root","ts":3,"category":category}));
            assert!(!normalized(&input).index_v2);
        }
        let mut input = frames();
        let mut different = input[1].clone();
        different["msg"] = json!(std::env::temp_dir().join("other.lock"));
        input.push(different);
        assert_eq!(
            normalized(&input).index_write,
            IndexWriteEvidence::Conflicting
        );
        for path in [json!("relative.lock"), json!("x".repeat(4097)), Value::Null] {
            let mut input = frames();
            input[1]["msg"] = path;
            input.push(frames()[1].clone());
            assert_eq!(
                normalized(&input).index_write,
                IndexWriteEvidence::Conflicting
            );
        }
    }

    #[test]
    fn reset_index_context_rejects_fsmonitor_and_shared_index_receipts_without_repo_id() {
        let receipts = [
            json!({"event":"data","category":"index","key":"extension/fsmn/read/token","value":"token"}),
            json!({"event":"data","category":"index","key":"extension/fsmn/write/token","value":"token"}),
            json!({"event":"region_enter","category":"fsm_hook","label":"query"}),
            json!({"event":"data","category":"fsm_client","key":"query/trivial-response","value":"1"}),
            json!({"event":"region_enter","category":"index","label":"shared/do_read_index"}),
            json!({"event":"region_enter","category":"index","label":"shared/do_write_index"}),
        ];
        for mut receipt in receipts {
            receipt["sid"] = json!("root");
            receipt["ts"] = json!(3);
            let mut input = frames();
            input.insert(1, receipt.clone());
            assert!(!normalized(&input).index_v2, "{receipt}");
            input[1]["sid"] = json!("root/child");
            assert!(normalized(&input).index_v2, "child receipt: {receipt}");
        }
    }
}
