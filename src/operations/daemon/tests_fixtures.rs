//! The `NormalizedCommand` fixture shared by the daemon's unit tests. The
//! analyzers are a pure layer that may not import the rest of the daemon, so
//! their tests keep their own base fixture in `analyzers/mod.rs`.
use crate::model::domain::{CommandScope, Confidence, NormalizedCommand};
use std::collections::HashMap;

/// A successful global-scope `primary` command with the given argv (starting
/// with `git`), no worktree, no family and no ref changes. Tests set the
/// fields they exercise with struct update syntax.
pub(crate) fn normalized_command(primary: &str, argv: &[&str]) -> NormalizedCommand {
    NormalizedCommand {
        scope: CommandScope::Global,
        family_key: None,
        worktree: None,
        root_sid: "r".to_string(),
        trace_derived: false,
        raw_argv: argv.iter().map(|arg| arg.to_string()).collect(),
        primary_command: Some(primary.to_string()),
        invoked_command: Some(primary.to_string()),
        invoked_args: Vec::new(),
        observed_child_commands: Vec::new(),
        transport_targets: None,
        index_write: Default::default(),
        index_v2: false,
        bisect_checkout: None,
        exit_code: 0,
        started_at_ns: 1,
        finished_at_ns: 2,
        reflog_start_offsets: HashMap::new(),
        stash_target_oid: None,
        cherry_pick_source_oids: Vec::new(),
        revert_source_oids: Vec::new(),
        ref_changes: Vec::new(),
        confidence: Confidence::Low,
    }
}
