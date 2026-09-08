use super::*;
use crate::model::jj_observer::JjObserverSessionCursor;

fn blocked_status() -> JjObserverControlReply {
    JjObserverControlReply {
        schema_version: 1,
        backend: "jj".to_owned(),
        attribution_enabled: false,
        action: "observer_status".to_owned(),
        disposition: "status".to_owned(),
        revision: Some(2),
        desired_intent: Some("enabled".to_owned()),
        target: Some(JjObserverTarget {
            source_id: "0".repeat(64),
            initialization_receipt_id: "1".repeat(64),
            reader_profile: JJ_OBSERVATION_READER_PROFILE.to_owned(),
            baseline_id: "2".repeat(64),
            baseline_generation: 1,
            workspace_name: "default".to_owned(),
            attachment_id: "3".repeat(64),
        }),
        runtime: "blocked".to_owned(),
        in_flight: false,
        session_cursor: Some(JjObserverSessionCursor {
            generation: 1,
            admitted_head_ids: vec!["4".repeat(128)],
        }),
        last_error: Some(JjObserverError {
            code: "admission_unavailable".to_owned(),
            message: "bounded diagnostic".to_owned(),
            persisted: true,
        }),
        error: None,
    }
}

#[test]
fn jj_observer_reply_accepts_empty_persisted_status_diagnostic() {
    let mut reply = blocked_status();
    validate(&reply, "observer_status", true).unwrap();
    reply.last_error.as_mut().unwrap().message.clear();
    validate(&reply, "observer_status", true).unwrap();
}

#[test]
fn jj_observer_reply_accepts_empty_command_error_diagnostic() {
    let mut reply = blocked_status();
    reply.action = "observer_resume".to_owned();
    reply.disposition = "error".to_owned();
    reply.error = Some(JjObserverError {
        code: "resume_required".to_owned(),
        message: "bounded diagnostic".to_owned(),
        persisted: false,
    });
    validate(&reply, "observer_resume", false).unwrap();
    reply.error.as_mut().unwrap().message.clear();
    validate(&reply, "observer_resume", false).unwrap();
}
