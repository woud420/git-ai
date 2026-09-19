use super::*;

impl RefCursor {
    pub(super) fn enrich_bisect(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        #[cfg(feature = "test-support")]
        eprintln!("bisect cursor command: {cmd:?}; refs: {:?}", state.refs);
        let Some(checkout) = cmd.bisect_checkout.as_ref() else {
            return Ok(());
        };
        if !cmd.trace_derived
            || cmd.exit_code != 0
            || cmd.observed_child_commands != ["checkout"]
            || checkout.started_at_ns < cmd.started_at_ns
            || checkout.finished_at_ns > cmd.finished_at_ns
            || checkout.started_at_ns > checkout.finished_at_ns
        {
            return Ok(());
        }
        let Some(git_dir) = cmd.worktree.as_deref().and_then(git_dir_for_worktree) else {
            return Ok(());
        };
        let path = git_dir.join("logs").join("HEAD");
        // A late start-offset hint can point beyond the child checkout. Read a
        // bounded log and require a unique, unconsumed child-window match instead.
        let Ok(Some(records)) = read_reflog_records_bounded(&path, 1024 * 1024) else {
            return Ok(());
        };
        let key = head_key(&git_dir);
        let consumed_through = self.reflog_start_offset(&key, &path)?.unwrap_or(0);
        #[cfg(feature = "test-support")]
        eprintln!("bisect cursor floor: {consumed_through}; records: {records:?}");
        let start_secs = checkout.started_at_ns / 1_000_000_000;
        let end_secs = checkout.finished_at_ns / 1_000_000_000;
        let mut matches = records.into_iter().filter_map(|record| {
            if record.start_offset < consumed_through {
                return None;
            }
            let timestamp = u128::try_from(record.timestamp_secs?).ok()?;
            if timestamp < start_secs
                || timestamp > end_secs
                || !record.message.starts_with("checkout: moving from ")
                || record.message.rsplit_once(" to ")?.1 != checkout.target
                || (is_valid_git_oid(&checkout.target) && record.new != checkout.target)
            {
                return None;
            }
            let entry = CursorEntry {
                key: key.clone(),
                path: path.clone(),
                reference: "HEAD".to_string(),
                old: record.old,
                new: record.new,
                message: record.message,
                timestamp_secs: record.timestamp_secs,
                start_offset: record.start_offset,
                end_offset: record.end_offset,
            };
            (!self.entry_consumed(&entry)).then_some(entry)
        });
        let Some(entry) = matches.next() else {
            return Ok(());
        };
        if matches.next().is_some()
            || entry.old == entry.new
            || !self.head_expected_transition(cmd, state).matches(&entry)
        {
            return Ok(());
        }
        self.consume_entry(&entry)?;
        cmd.ref_changes.push(entry_to_ref_change(&entry));
        Ok(())
    }
}
