use crate::model::jj_observation::{
    JJ_OBSERVATION_READER_PROFILE, MAX_JJ_OBSERVATION_HEADS, is_root, validate_ids, validate_source,
};
use crate::model::jj_observer::{JjObserverControlReply, JjObserverError, JjObserverTarget};
use crate::model::repository::jj_observation_journal::validate_workspace_name;

pub(super) fn validate(value: &JjObserverControlReply, action: &str, ok: bool) -> Result<(), ()> {
    if value.schema_version != 1
        || value.backend != "jj"
        || value.attribution_enabled
        || value.action != action
        || ok != value.error.is_none()
        || !matches!(
            value.runtime.as_str(),
            "pending" | "active" | "stopping" | "blocked" | "disabled"
        )
        || !matches!(
            value.desired_intent.as_deref(),
            None | Some("enabled" | "disabled")
        )
    {
        return Err(());
    }
    let disposition = value.disposition.as_str();
    if value.error.is_some() {
        if disposition != "error" {
            return Err(());
        }
    } else if !match action {
        "observer_enable" => matches!(
            disposition,
            "enabled" | "already_enabled" | "resume_started"
        ),
        "observer_status" => disposition == "status",
        "observer_disable" => matches!(disposition, "disable_requested" | "already_disabled"),
        "observer_resume" => matches!(disposition, "resume_started" | "already_active"),
        _ => false,
    } {
        return Err(());
    }
    if let Some(target) = &value.target {
        validate_target(target)?;
    }
    if let Some(cursor) = &value.session_cursor {
        if cursor.generation > i64::MAX as u64
            || cursor.admitted_head_ids.is_empty()
            || cursor.admitted_head_ids.len() > MAX_JJ_OBSERVATION_HEADS
            || cursor.admitted_head_ids.iter().any(|head| is_root(head))
        {
            return Err(());
        }
        validate_ids(&cursor.admitted_head_ids, MAX_JJ_OBSERVATION_HEADS).map_err(|_| ())?;
    }
    if value.runtime == "active"
        && (value.target.is_none()
            || value.session_cursor.is_none()
            || value.revision.is_none()
            || value.desired_intent.as_deref() != Some("enabled"))
    {
        return Err(());
    }
    if value.runtime == "disabled" && (value.in_flight || value.session_cursor.is_some()) {
        return Err(());
    }
    for error in [&value.error, &value.last_error].into_iter().flatten() {
        validate_error(error)?;
    }
    Ok(())
}

fn validate_target(target: &JjObserverTarget) -> Result<(), ()> {
    for id in [
        &target.source_id,
        &target.initialization_receipt_id,
        &target.baseline_id,
        &target.attachment_id,
    ] {
        validate_source(id).map_err(|_| ())?;
    }
    if target.reader_profile != JJ_OBSERVATION_READER_PROFILE || target.baseline_generation != 1 {
        return Err(());
    }
    validate_workspace_name(&target.workspace_name).map_err(|_| ())
}

fn validate_error(error: &JjObserverError) -> Result<(), ()> {
    if error.code.is_empty()
        || error.code.len() > 64
        || !error
            .code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || error.message.len() > 4096
        || error.message.chars().count() > 1024
    {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
#[path = "reply_tests.rs"]
mod tests;
