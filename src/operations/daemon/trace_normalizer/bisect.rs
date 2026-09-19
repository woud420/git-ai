use crate::model::domain::BisectCheckout;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub(super) struct BisectCheckoutCapture {
    enabled: bool,
    invalid: bool,
    root_worktree: Option<String>,
    child_worktree: Option<String>,
    child_sid: Option<String>,
    target: Option<String>,
    named_checkout: bool,
    started_at_ns: u128,
    finished_at_ns: Option<u128>,
    parent_child_id: Option<u64>,
    parent_target: Option<String>,
    parent_started_at_ns: u128,
    parent_finished_at_ns: Option<u128>,
}

impl BisectCheckoutCapture {
    pub(super) fn new(argv: &[String], worktree: Option<&std::path::Path>) -> Self {
        let (command, args) = super::frame_helpers::canonical_invocation(argv, None);
        Self {
            enabled: command.as_deref() == Some("bisect")
                && args.first().is_some_and(|arg| {
                    matches!(
                        arg.as_str(),
                        "start" | "good" | "bad" | "old" | "new" | "skip" | "next" | "reset"
                    )
                }),
            root_worktree: worktree.map(|path| path.to_string_lossy().into_owned()),
            ..Self::default()
        }
    }

    pub(super) fn observe(&mut self, payload: &Value, sid: &str, root: &str, timestamp: u128) {
        if !self.enabled || self.invalid {
            return;
        }
        let event = payload["event"].as_str().unwrap_or_default();
        if event == "def_repo" && payload["repo"].as_u64() == Some(1) {
            if sid == root {
                self.root_worktree = payload["worktree"].as_str().map(str::to_string);
            } else if self.child_sid.as_deref() == Some(sid) {
                self.child_worktree = payload["worktree"].as_str().map(str::to_string);
            }
        }
        if sid == root {
            self.observe_parent(payload, event, timestamp);
            return;
        }
        match event {
            "start" => {
                // More than one child or a hook's nested Git command cannot be
                // represented by this single-checkout receipt.
                if self.child_sid.is_some()
                    || sid
                        .strip_prefix(root)
                        .is_none_or(|tail| tail.matches('/').count() != 1)
                {
                    self.invalid = true;
                    return;
                }
                let argv = crate::operations::daemon::trace_helpers::trace_payload_argv(payload);
                let Some(target) = checkout_target(&argv) else {
                    self.invalid = true;
                    return;
                };
                self.child_sid = Some(sid.to_string());
                self.target = Some(target.to_string());
                self.started_at_ns = timestamp;
            }
            "cmd_name" if self.child_sid.as_deref() == Some(sid) => {
                self.named_checkout = payload["name"].as_str() == Some("checkout");
                self.invalid |= !self.named_checkout;
            }
            "exit" if self.child_sid.as_deref() == Some(sid) => {
                self.invalid |= payload["code"].as_i64() != Some(0);
                self.finished_at_ns = Some(timestamp);
            }
            _ => {}
        }
    }

    fn observe_parent(&mut self, payload: &Value, event: &str, timestamp: u128) {
        match event {
            "child_start" => {
                let argv = crate::operations::daemon::trace_helpers::trace_payload_argv(payload);
                let child_id = payload["child_id"].as_u64();
                let target = checkout_target(&argv);
                if self.parent_child_id.is_some()
                    || child_id.is_none()
                    || target.is_none()
                    || argv.first().map(String::as_str) != Some("git")
                    || payload["use_shell"].as_bool() != Some(false)
                    || payload.get("cd").is_some()
                {
                    self.invalid = true;
                    return;
                }
                self.parent_child_id = child_id;
                self.parent_target = target.map(str::to_string);
                self.parent_started_at_ns = timestamp;
            }
            "child_exit" => {
                self.invalid |= self.parent_child_id.is_none()
                    || self.parent_child_id != payload["child_id"].as_u64()
                    || payload["code"].as_i64() != Some(0)
                    || self.parent_finished_at_ns.is_some();
                self.parent_finished_at_ns = Some(timestamp);
            }
            _ => {}
        }
    }

    pub(super) fn receipt(&self, exit_code: i32) -> Option<BisectCheckout> {
        if !self.enabled
            || self.invalid
            || exit_code != 0
            || self.root_worktree.is_none()
            || (self.child_worktree.is_some() && self.root_worktree != self.child_worktree)
        {
            return None;
        }
        if let Some(target) = &self.parent_target {
            let finished_at_ns = self.parent_finished_at_ns?;
            if self.parent_started_at_ns == 0
                || self.parent_started_at_ns > finished_at_ns
                || self.target.as_ref().is_some_and(|child| child != target)
                || (self.child_sid.is_some()
                    && (self.started_at_ns < self.parent_started_at_ns
                        || self.started_at_ns > finished_at_ns))
                || self
                    .finished_at_ns
                    .is_some_and(|child| child < self.started_at_ns || child > finished_at_ns)
            {
                return None;
            }
            // Git's built-in bisect launches this checkout without changing cwd.
            // Its parent stream records completion before root exit even when the
            // separate child stream has not reached the daemon yet.
            return Some(BisectCheckout {
                target: target.clone(),
                started_at_ns: self.parent_started_at_ns,
                finished_at_ns,
            });
        }
        if !self.named_checkout || self.root_worktree != self.child_worktree {
            return None;
        }
        let finished_at_ns = self.finished_at_ns?;
        if self.started_at_ns == 0 || self.started_at_ns > finished_at_ns {
            return None;
        }
        Some(BisectCheckout {
            target: self.target.clone()?,
            started_at_ns: self.started_at_ns,
            finished_at_ns,
        })
    }
}

fn checkout_target(argv: &[String]) -> Option<&str> {
    match argv {
        [_, command, option, target, separator]
            if command == "checkout"
                && matches!(option.as_str(), "-q" | "--ignore-other-worktrees")
                && separator == "--"
                && !target.is_empty()
                && !target.starts_with('-')
                && target.len() <= 4096 =>
        {
            Some(target)
        }
        _ => None,
    }
}
