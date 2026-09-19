use super::{GitBackend, TraceNormalizer};
use crate::operations::daemon::trace_normalizer::PendingTraceCommand;
use serde_json::Value;
use std::path::Path;

impl<B: GitBackend> TraceNormalizer<B> {
    pub(in crate::operations::daemon::trace_normalizer) fn observe_push_alias_name(
        &mut self,
        payload: &Value,
        sid: &str,
        root: &str,
    ) {
        let Some(child) = sid
            .strip_prefix(root)
            .and_then(|tail| tail.strip_prefix('/'))
        else {
            return;
        };
        if child.is_empty()
            || child.contains('/')
            || payload.get("name").and_then(Value::as_str) != Some("push")
        {
            return;
        }
        let Some(pending) = self.state.pending.get_mut(root) else {
            return;
        };
        if pending.root_cmd_name.as_deref() == Some("push")
            || crate::operations::daemon::trace_helpers::trace_argv_primary_command(
                &pending.raw_argv,
            )
            .as_deref()
                == Some("push")
        {
            return;
        }
        match pending.push_alias_sid.as_deref() {
            None => pending.push_alias_sid = Some(sid.to_owned()),
            Some(owner) if owner == sid => {}
            Some(_) => pending.push_alias_targets = None,
        }
    }
}

pub(super) fn capture_parent(
    pending: &mut PendingTraceCommand,
    payload: &Value,
    sid: &str,
    root: &str,
    class: &str,
) -> bool {
    if sid == root && class == "git_alias" {
        let argv = crate::operations::daemon::trace_helpers::trace_payload_argv(payload);
        if pending.push_alias.is_some() {
            pending.push_alias_targets = None;
        }
        pending.push_alias = Some(
            payload.get("use_shell").and_then(Value::as_bool) == Some(false)
                && matches!(argv.as_slice(), [git, command, ..]
                if Path::new(git).file_name().and_then(|name| name.to_str()).is_some_and(|name| matches!(name, "git" | "git.exe"))
                    && command == "push"),
        );
        return true;
    }
    false
}

pub(in crate::operations::daemon::trace_normalizer) fn finish(
    pending: &mut PendingTraceCommand,
) -> Option<Vec<String>> {
    // Parent and child connections can be drained in either order. A child's
    // bounded candidate becomes a destination only after its parent authorizes
    // the plain push alias; nested, ambiguous and unsupported children stay out.
    if pending.push_alias == Some(true) {
        if pending
            .transport_targets
            .as_ref()
            .is_some_and(Vec::is_empty)
        {
            pending.push_alias_targets.take()
        } else {
            None
        }
    } else {
        pending.transport_targets.take()
    }
}
