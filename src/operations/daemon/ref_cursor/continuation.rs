use super::*;

impl RefCursor {
    pub(super) fn consume_commit_continuation(
        &mut self,
        cmd: &mut NormalizedCommand,
    ) -> Result<(), GitAiError> {
        let Some(worktree) = cmd.worktree.as_deref() else {
            return Ok(());
        };
        let Some(git_dir) = git_dir_for_worktree(worktree) else {
            return Ok(());
        };
        let key = head_key(&git_dir);
        let path = git_dir.join("logs/HEAD");
        let Some(start) = self.reflog_start_offset(&key, &path)? else {
            return Ok(());
        };
        let prefixes: &[&str] = if cmd.primary_command.as_deref() == Some("merge") {
            &["commit (merge):"]
        } else {
            &["commit:", "commit (revert):", "revert:"]
        };
        let window = reflog_timestamp_window(cmd);
        let Some(old_head) = self.anchors.get(&key).map(|anchor| anchor.new.clone()) else {
            return Ok(());
        };
        let candidates: Vec<_> = read_reflog_entries(key, &path, "HEAD", Some(start))?
            .into_iter()
            .filter(|entry| {
                !self.entry_consumed(entry)
                    && message_matches(&entry.message, prefixes)
                    && entry
                        .timestamp_secs
                        .is_some_and(|time| window.contains(time))
            })
            .collect();
        // A resumed sequence may create several commits. Do not attribute its
        // final checkpoint state to an arbitrary first commit in that sequence.
        let [entry] = candidates.as_slice() else {
            return Ok(());
        };
        if entry.old != old_head || !valid_non_zero_oid(&entry.new) {
            return Ok(());
        }
        self.consume_head_entry_for_command(cmd, entry.clone())
    }
}
