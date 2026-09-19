use super::{GitBackend, TraceNormalizer};
use serde_json::Value;
use std::path::Path;

mod ssh;

const MAX_TARGETS: usize = 8;
const MAX_TARGET_BYTES: usize = 32 * 1024;

impl<B: GitBackend> TraceNormalizer<B> {
    pub(super) fn observe_push_alias_name(&mut self, payload: &Value, sid: &str, root: &str) {
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
        if !pending.push_alias {
            return;
        }
        match pending.push_alias_sid.as_deref() {
            None => pending.push_alias_sid = Some(sid.to_owned()),
            Some(owner) if owner == sid => {}
            Some(_) => pending.transport_targets = None,
        }
    }

    pub(super) fn capture_transport_target(&mut self, payload: &Value, sid: &str, root: &str) {
        let Some(pending) = self.state.pending.get_mut(root) else {
            return;
        };
        let Some(class) = payload.get("child_class").and_then(Value::as_str) else {
            return;
        };
        if sid == root
            && class == "git_alias"
            && payload.get("use_shell").and_then(Value::as_bool) == Some(false)
        {
            let argv = crate::operations::daemon::trace_helpers::trace_payload_argv(payload);
            pending.push_alias = matches!(argv.as_slice(), [git, command, ..]
                if Path::new(git).file_name().and_then(|name| name.to_str()).is_some_and(|name| matches!(name, "git" | "git.exe"))
                    && command == "push");
            if pending.push_alias_sid.is_some() {
                pending.transport_targets = None;
            }
            return;
        }
        if sid != root {
            if pending.push_alias_sid.as_deref() != Some(sid) {
                return;
            }
        } else if pending.root_cmd_name.as_deref() != Some("push")
            && crate::operations::daemon::trace_helpers::trace_argv_primary_command(
                &pending.raw_argv,
            )
            .as_deref()
                != Some("push")
        {
            return;
        }
        if !class.starts_with("transport/") && !class.starts_with("remote-") {
            return;
        }
        let Some(targets) = pending.transport_targets.as_mut() else {
            return;
        };
        let target = target_from_frame(payload, class, pending.worktree.as_deref());
        match target {
            Some(target) if targets.contains(&target) => {}
            Some(target) if targets.len() < MAX_TARGETS => targets.push(target),
            _ => pending.transport_targets = None,
        }
    }
}

fn target_from_frame(payload: &Value, class: &str, worktree: Option<&Path>) -> Option<String> {
    let argv = payload.get("argv")?.as_array()?;
    let target = match class {
        "transport/file" => {
            let [command] = argv.as_slice() else {
                return None;
            };
            let path = receive_pack_path(command.as_str()?)?;
            let path = Path::new(&path);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                // Git runs this service from the primary repository root. Keep
                // parent components: lexical normalization would change symlink traversal.
                worktree.filter(|root| root.is_absolute())?.join(path)
            }
            .to_str()?
            .to_owned()
        }
        "transport/ssh" => ssh::destination(argv)?,
        "remote-http" | "remote-https" => {
            let [git, helper, _, destination] = argv.as_slice() else {
                return None;
            };
            let git = Path::new(git.as_str()?).file_name()?.to_str()?;
            if !matches!(git, "git" | "git.exe") || helper.as_str()? != class {
                return None;
            }
            let destination = destination.as_str()?;
            if !destination.starts_with("http://") && !destination.starts_with("https://") {
                return None;
            }
            destination.to_owned()
        }
        _ => return None,
    };
    (!target.is_empty() && target.len() <= MAX_TARGET_BYTES && !target.contains('\0'))
        .then_some(target)
}

fn receive_pack_path(command: &str) -> Option<String> {
    let mut rest = command.strip_prefix("git-receive-pack ")?;
    let mut path = String::new();
    // Accept only Git's single-quoted path encoding, including embedded quotes.
    // Never interpret arbitrary shell syntax or custom receive-pack commands.
    while !rest.is_empty() {
        rest = rest.strip_prefix('\'')?;
        let end = rest.find('\'')?;
        path.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        if !rest.is_empty() {
            rest = rest.strip_prefix("\\'")?;
            path.push('\'');
        }
    }
    (!path.is_empty() && path.len() <= MAX_TARGET_BYTES).then_some(path)
}

#[cfg(test)]
mod tests;
