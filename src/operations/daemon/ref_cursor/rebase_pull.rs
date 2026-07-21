use super::*;

impl RefCursor {
    pub(super) fn consume_rebase_transition(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        if cmd.exit_code != 0 && self.consume_failed_explicit_branch_rebase_start(cmd)? {
            return Ok(());
        }

        let expected = self.head_expected_transition(cmd, state);
        let first = match self.find_rebase_start_entry(cmd, expected.clone())? {
            Some(entry) => Some(entry),
            None => {
                self.find_head_entry_without_hint(cmd.worktree.as_deref(), &["rebase"], expected)?
            }
        };
        let Some(first) = first else {
            return Ok(());
        };

        let mut changes = vec![entry_to_ref_change(&first)];
        let old = first.old.clone();
        let mut new = first.new.clone();
        self.consume_entry(&first)?;

        let failed = cmd.exit_code != 0;
        if failed {
            cmd.ref_changes = changes;
            return Ok(());
        }

        let mut consumed_finish = rebase_reflog_action_is(&first.message, "finish");
        let mut next_start = first.end_offset;
        while !consumed_finish {
            let Some(next) = self.find_head_entry_after(
                cmd.worktree.as_deref(),
                next_start,
                &["rebase"],
                ExpectedTransition {
                    old_oids: [new.clone()].into_iter().collect(),
                    new_oid: None,
                    messages: HashSet::new(),
                },
            )?
            else {
                break;
            };
            if rebase_reflog_action_is(&next.message, "start") {
                break;
            }
            new = next.new.clone();
            consumed_finish = rebase_reflog_action_is(&next.message, "finish");
            next_start = next.end_offset;
            self.consume_entry(&next)?;
            changes.push(entry_to_ref_change(&next));
        }

        self.consume_common_refs_matching_transition(&old, &new, &mut changes)?;
        self.consume_rebase_finish_branch_ref(cmd.worktree.as_deref(), &new, state, &mut changes)?;
        dedup_ref_changes(&mut changes);
        cmd.ref_changes = changes;
        Ok(())
    }

    // DEFERRED (code-review #14): this finder scans the reflog from
    // reflog_start_offset and takes the first unconsumed entry matching the
    // message/transition; it does not use the command's ingress hint (the
    // reflog position at which THIS command began) to bound the search. In
    // pathological histories with repeated identical rebase-start messages and
    // transitions, it could match an earlier same-shaped entry than the one
    // this command produced. Bounding by the per-command ingress offset would
    // make the match exact; deferred as it needs the ingress offset threaded
    // through to the finder.
    pub(super) fn find_rebase_start_entry(
        &mut self,
        cmd: &NormalizedCommand,
        expected: ExpectedTransition,
    ) -> Result<Option<CursorEntry>, GitAiError> {
        let Some(worktree) = cmd.worktree.as_deref() else {
            return Ok(None);
        };
        let Some(git_dir) = git_dir_for_worktree(worktree) else {
            return Ok(None);
        };
        let args = rebase_command_args(cmd);
        let target = rebase_start_checkout_target_from_args(&args);
        let key = head_key(&git_dir);
        let path = git_dir.join("logs").join("HEAD");
        let start = self.reflog_start_offset(&key, &path)?;
        let entries = read_reflog_entries(key, &path, "HEAD", start)?;

        Ok(entries.into_iter().find(|entry| {
            !self.entry_consumed(entry)
                && rebase_reflog_action_is(&entry.message, "start")
                && expected.matches_span_boundary(entry)
                && target
                    .as_deref()
                    .is_none_or(|target| rebase_start_message_targets(&entry.message, target))
        }))
    }

    pub(super) fn consume_failed_explicit_branch_rebase_start(
        &mut self,
        cmd: &mut NormalizedCommand,
    ) -> Result<bool, GitAiError> {
        let args = rebase_command_args(cmd);
        let Some(branch_arg) = explicit_rebase_branch_arg(&args) else {
            return Ok(false);
        };
        let Some(worktree) = cmd.worktree.as_deref() else {
            return Ok(false);
        };
        let Some(git_dir) = git_dir_for_worktree(worktree) else {
            return Ok(false);
        };

        let branch_ref = branch_arg_to_ref(&branch_arg);
        let head_key = head_key(&git_dir);
        let head_path = git_dir.join("logs").join("HEAD");
        let start = self.reflog_start_offset(&head_key, &head_path)?;
        let head_entries =
            read_reflog_entries_including_noops(head_key, &head_path, "HEAD", start)?;
        let Some(start_marker) =
            rebase_start_marker_for_explicit_branch(&head_entries, &branch_ref)
        else {
            return Ok(false);
        };

        let finish_new = latest_rebase_finish_for_branch(&head_entries, &branch_ref)
            .filter(|finish| finish.end_offset > start_marker.end_offset)
            .map(|finish| finish.new.as_str());
        let original_head =
            self.original_head_for_explicit_rebase_branch(&branch_ref, finish_new)?;

        let mut changes = vec![entry_to_ref_change(start_marker)];
        if let Some(original_head) = original_head {
            changes.push(RefChange {
                reference: branch_ref,
                old: original_head.clone(),
                new: original_head,
            });
        }

        self.advance_cursor_to_entry(start_marker);
        dedup_ref_changes(&mut changes);
        cmd.ref_changes = changes;
        Ok(true)
    }

    pub(super) fn consume_pull_transition(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        let action = pull_reflog_action(cmd);
        let prefixes = pull_reflog_message_prefixes(&action);
        let prefix_refs = prefixes.iter().map(String::as_str).collect::<Vec<_>>();
        let expected = self.head_expected_transition(cmd, state);
        self.consume_pull_head_span_for_action(cmd, state, &prefix_refs, expected, &action)
    }

    pub(super) fn consume_pull_head_span_for_action(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
        message_prefixes: &[&str],
        expected: ExpectedTransition,
        action: &str,
    ) -> Result<(), GitAiError> {
        let first = match self.find_pull_start_entry(cmd, expected.clone(), action)? {
            Some(entry) => Some(entry),
            None => self.find_head_entry_without_hint(
                cmd.worktree.as_deref(),
                message_prefixes,
                expected,
            )?,
        };
        let Some(first) = first else {
            return Ok(());
        };

        let old = first.old.clone();
        let mut new = first.new.clone();
        let mut changes = vec![entry_to_ref_change(&first)];
        let mut consumed_finish = pull_reflog_action_state(&first.message, action).is_none()
            || pull_reflog_action_is(&first.message, action, "finish");
        let mut next_start = first.end_offset;
        self.consume_entry(&first)?;

        while !consumed_finish
            && let Some(next) = self.find_head_entry_after(
                cmd.worktree.as_deref(),
                next_start,
                message_prefixes,
                ExpectedTransition {
                    old_oids: [new.clone()].into_iter().collect(),
                    new_oid: None,
                    messages: HashSet::new(),
                },
            )?
        {
            if pull_reflog_action_starts_new_command(&next.message, action) {
                break;
            }
            new = next.new.clone();
            consumed_finish = pull_reflog_action_is(&next.message, action, "finish");
            next_start = next.end_offset;
            self.consume_entry(&next)?;
            changes.push(entry_to_ref_change(&next));
        }

        self.consume_common_refs_matching_transition(&old, &new, &mut changes)?;
        self.consume_pull_finish_branch_ref(
            cmd.worktree.as_deref(),
            &new,
            state,
            action,
            message_prefixes,
            &mut changes,
        )?;
        dedup_ref_changes(&mut changes);
        cmd.ref_changes = changes;
        Ok(())
    }

    pub(super) fn find_pull_start_entry(
        &mut self,
        cmd: &NormalizedCommand,
        expected: ExpectedTransition,
        action: &str,
    ) -> Result<Option<CursorEntry>, GitAiError> {
        let Some(worktree) = cmd.worktree.as_deref() else {
            return Ok(None);
        };
        let Some(git_dir) = git_dir_for_worktree(worktree) else {
            return Ok(None);
        };
        let key = head_key(&git_dir);
        let path = git_dir.join("logs").join("HEAD");
        let start = self.reflog_start_offset(&key, &path)?;
        let entries = read_reflog_entries(key, &path, "HEAD", start)?;

        Ok(entries.into_iter().find(|entry| {
            !self.entry_consumed(entry)
                && pull_reflog_action_is(&entry.message, action, "start")
                && expected.matches_span_boundary(entry)
        }))
    }

    pub(super) fn consume_rebase_finish_branch_ref(
        &mut self,
        worktree: Option<&Path>,
        new: &str,
        state: &FamilyState,
        out: &mut Vec<RefChange>,
    ) -> Result<(), GitAiError> {
        self.consume_finish_branch_ref(
            worktree,
            new,
            state,
            &["rebase"],
            |message| rebase_finish_returned_branch(message).map(ToOwned::to_owned),
            out,
        )
    }

    pub(super) fn consume_pull_finish_branch_ref(
        &mut self,
        worktree: Option<&Path>,
        new: &str,
        state: &FamilyState,
        action: &str,
        message_prefixes: &[&str],
        out: &mut Vec<RefChange>,
    ) -> Result<(), GitAiError> {
        self.consume_finish_branch_ref(
            worktree,
            new,
            state,
            message_prefixes,
            |message| pull_finish_returned_branch(message, action),
            out,
        )
    }

    pub(super) fn consume_finish_branch_ref<F>(
        &mut self,
        worktree: Option<&Path>,
        new: &str,
        state: &FamilyState,
        message_prefixes: &[&str],
        branch_from_message: F,
        out: &mut Vec<RefChange>,
    ) -> Result<(), GitAiError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let Some(worktree) = worktree else {
            return Ok(());
        };
        let Some(git_dir) = git_dir_for_worktree(worktree) else {
            return Ok(());
        };
        let key = head_key(&git_dir);
        let path = git_dir.join("logs").join("HEAD");
        let start = self.reflog_start_offset(&key, &path)?;
        let entries = read_reflog_entries_including_noops(key, &path, "HEAD", start)?;
        let Some(finish) = entries
            .into_iter()
            .find(|entry| entry.new == new && branch_from_message(&entry.message).is_some())
        else {
            return Ok(());
        };
        let Some(branch_ref) = branch_from_message(&finish.message) else {
            return Ok(());
        };

        self.advance_cursor_to_entry(&finish);
        let old_oids = state
            .refs
            .get(&branch_ref)
            .filter(|oid| valid_non_zero_oid(oid))
            .cloned()
            .into_iter()
            .collect();
        if let Some(entry) = self.find_common_ref_entry(
            &branch_ref,
            ExpectedTransition {
                old_oids,
                new_oid: Some(new.to_string()),
                messages: HashSet::new(),
            },
            message_prefixes,
        )? {
            self.consume_entry(&entry)?;
            out.push(entry_to_ref_change(&entry));
        }
        Ok(())
    }

    pub(super) fn original_head_for_explicit_rebase_branch(
        &mut self,
        branch_ref: &str,
        finished_new: Option<&str>,
    ) -> Result<Option<String>, GitAiError> {
        let path = self.common_dir().join("logs").join(branch_ref);
        let key = common_key(branch_ref);
        let start = self.reflog_start_offset(&key, &path)?;
        let entries = read_reflog_entries(key.clone(), &path, branch_ref, start)?;

        if let Some(finished_new) = finished_new
            && let Some(entry) = entries.iter().rev().find(|entry| {
                entry.new == finished_new
                    && rebase_branch_finish_message_is(&entry.message, branch_ref)
                    && valid_non_zero_oid(&entry.old)
            })
        {
            return Ok(Some(entry.old.clone()));
        }

        let latest_entry_tip = entries
            .iter()
            .rev()
            .find(|entry| valid_non_zero_oid(&entry.new))
            .map(|entry| entry.new.clone());
        Ok(latest_entry_tip.or_else(|| self.ref_tip_at_cursor_start(&key)))
    }
}
