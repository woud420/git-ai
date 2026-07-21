use super::*;

impl RefCursor {
    pub(super) fn enrich_update_ref(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        let args = command_args(cmd);
        let spec = parse_update_ref_spec(&args)?;
        let Some(spec) = spec else {
            let mut changes = Vec::new();
            if let Some(worktree) = cmd.worktree.as_deref() {
                while let Some(entry) = self.find_head_entry_without_hint(
                    Some(worktree),
                    &[],
                    ExpectedTransition::default(),
                )? {
                    self.consume_entry(&entry)?;
                    changes.push(entry_to_ref_change(&entry));
                }
            }
            for reference in self.discover_common_refs()? {
                if reference == "HEAD" || reference == "ORIG_HEAD" {
                    continue;
                }
                while let Some(entry) = self.find_common_ref_entry_without_hint(
                    &reference,
                    ExpectedTransition::default(),
                    &[],
                )? {
                    self.consume_entry(&entry)?;
                    changes.push(entry_to_ref_change(&entry));
                }
            }
            dedup_ref_changes(&mut changes);
            cmd.ref_changes = changes;
            return Ok(());
        };

        let mut changes = Vec::new();
        if spec.reference == "HEAD" {
            if let Some(change) = direct_update_ref_change_from_argv(&spec) {
                changes.push(change.clone());
                if let Some(head) =
                    self.find_direct_ref_transition_entry(cmd, "HEAD", &change.old, &change.new)?
                {
                    let head_timestamp = head.timestamp_secs;
                    self.consume_entry(&head)?;
                    if let Some(timestamp) = head_timestamp {
                        if let Some(branch) = current_worktree_branch_ref(cmd, state) {
                            if let Some(entry) = self
                                .direct_ref_transition_entries(
                                    cmd,
                                    branch,
                                    &change.old,
                                    &change.new,
                                )?
                                .into_iter()
                                .find(|entry| entry.timestamp_secs == Some(timestamp))
                            {
                                self.consume_entry(&entry)?;
                                changes.push(entry_to_ref_change(&entry));
                            }
                        } else {
                            self.consume_unique_direct_common_ref_matching_timestamp(
                                cmd,
                                &change.old,
                                &change.new,
                                timestamp,
                                &mut changes,
                            )?;
                        }
                    }
                } else {
                    self.consume_common_refs_matching_transition(
                        &change.old,
                        &change.new,
                        &mut changes,
                    )?;
                }
            } else if let Some(entry) = self.find_head_entry(
                cmd.worktree.as_deref(),
                &[],
                ExpectedTransition {
                    old_oids: spec.old_oid.iter().cloned().collect(),
                    new_oid: Some(spec.new_oid.clone()),
                    messages: HashSet::new(),
                },
            )? {
                self.consume_entry(&entry)?;
                changes.push(entry_to_ref_change(&entry));
                self.consume_common_refs_matching_transition(&entry.old, &entry.new, &mut changes)?;
            }
        } else if let Some(change) = direct_update_ref_change_from_argv(&spec) {
            let old = change.old.clone();
            let new = change.new.clone();
            changes.push(change);
            if let Some(branch) =
                self.find_direct_ref_transition_entry(cmd, &spec.reference, &old, &new)?
            {
                let branch_timestamp = branch.timestamp_secs;
                self.consume_entry(&branch)?;
                let branch_can_affect_head = current_worktree_branch_ref(cmd, state)
                    .is_none_or(|current_branch| current_branch == spec.reference);
                if branch_can_affect_head
                    && let Some(timestamp) = branch_timestamp
                    && let Some(head) = self
                        .direct_ref_transition_entries(cmd, "HEAD", &old, &new)?
                        .into_iter()
                        .find(|entry| entry.timestamp_secs == Some(timestamp))
                {
                    self.consume_entry(&head)?;
                    changes.push(entry_to_ref_change(&head));
                }
            }
        } else if let Some(entry) = self.find_common_ref_entry(
            &spec.reference,
            ExpectedTransition {
                old_oids: spec.old_oid.iter().cloned().collect(),
                new_oid: Some(spec.new_oid.clone()),
                messages: HashSet::new(),
            },
            &[],
        )? {
            self.consume_entry(&entry)?;
            let old = entry.old.clone();
            let new = entry.new.clone();
            changes.push(entry_to_ref_change(&entry));
            if let Some(head) = self.find_head_entry(
                cmd.worktree.as_deref(),
                &[],
                ExpectedTransition {
                    old_oids: [old.clone()].into_iter().collect(),
                    new_oid: Some(new.clone()),
                    messages: HashSet::new(),
                },
            )? {
                self.consume_entry(&head)?;
                changes.push(entry_to_ref_change(&head));
            }
        }

        dedup_ref_changes(&mut changes);
        cmd.ref_changes = changes;
        Ok(())
    }

    pub(super) fn enrich_stash(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        let args = command_args(cmd);
        let stash_args = stash_command_args(&args);
        let kind = stash_args.first().map(String::as_str).unwrap_or("push");

        if matches!(kind, "apply" | "pop" | "drop" | "branch") {
            let target = if kind == "branch" {
                stash_args.get(2)
            } else {
                stash_args.get(1)
            };
            cmd.stash_target_oid = self.resolve_stash_target_at_cursor(target)?;
        }

        if matches!(kind, "push" | "save") {
            if let Some(entry) = self.find_stash_push_entry(stash_args, kind)? {
                self.consume_entry(&entry)?;
                self.apply_stash_ref_entry(kind, &entry);
                cmd.ref_changes.push(entry_to_ref_change(&entry));
            }
        } else if matches!(kind, "pop" | "drop") {
            self.consume_destructive_stash_operation(stash_args.get(1), cmd)?;
        }

        if matches!(kind, "apply" | "pop" | "branch")
            && (kind == "branch" || !state.refs.contains_key("HEAD"))
        {
            let expected = if kind == "branch" {
                self.head_expected_transition(cmd, state)
            } else {
                ExpectedTransition::default()
            };
            if let Some(head) = self.find_head_entry(cmd.worktree.as_deref(), &[], expected)?
                && message_matches(&head.message, &["reset:", "checkout:"])
            {
                self.consume_entry(&head)?;
                cmd.ref_changes.push(entry_to_ref_change(&head));
            }
        }

        Ok(())
    }

    pub(super) fn consume_destructive_stash_operation(
        &mut self,
        target: Option<&String>,
        cmd: &mut NormalizedCommand,
    ) -> Result<(), GitAiError> {
        let key = common_key("refs/stash");
        let old_cursor = self.offsets.get(&key).copied();
        let log_len_after = self.common_ref_log_len("refs/stash")?;
        let log_was_rewritten = match (old_cursor, log_len_after) {
            (Some(cursor), Some(len)) => len < cursor,
            (Some(_), None) => true,
            _ => false,
        };

        if !log_was_rewritten {
            return Ok(());
        }

        let target_oid = cmd
            .stash_target_oid
            .clone()
            .or_else(|| self.resolve_stash_target_at_cursor(target).ok().flatten());
        let Some(target_oid) = target_oid else {
            self.sync_common_ref_cursor_to_log_end_after_rewrite("refs/stash")?;
            return Ok(());
        };

        let target_index = stash_target_index(target);
        let old_top = self.stash_stack.first().cloned();
        self.remove_stash_from_stack(target_index, &target_oid);
        let new_top = self.stash_stack.first().cloned().unwrap_or_else(zero_oid);

        if old_top.as_deref() == Some(target_oid.as_str()) {
            cmd.ref_changes.push(RefChange {
                reference: "refs/stash".to_string(),
                old: target_oid.clone(),
                new: new_top,
            });
        }
        if cmd.stash_target_oid.is_none() {
            cmd.stash_target_oid = Some(target_oid);
        }

        self.sync_common_ref_cursor_to_log_end_after_rewrite("refs/stash")?;
        Ok(())
    }

    pub(super) fn find_stash_push_entry(
        &mut self,
        stash_args: &[String],
        kind: &str,
    ) -> Result<Option<CursorEntry>, GitAiError> {
        let expected_message = stash_push_message_from_args(stash_args, kind);
        let path = self.common_dir().join("logs").join("refs/stash");
        let key = common_key("refs/stash");
        let start = self.reflog_start_offset(&key, &path)?;
        let entries = read_reflog_entries(key, &path, "refs/stash", start)?;

        Ok(entries.into_iter().find(|entry| {
            !self.entry_consumed(entry)
                && expected_message
                    .as_deref()
                    .is_none_or(|message| stash_reflog_message_matches(&entry.message, message))
        }))
    }

    pub(super) fn resolve_stash_target_at_cursor(
        &self,
        target: Option<&String>,
    ) -> Result<Option<String>, GitAiError> {
        let target = target.map(String::as_str).unwrap_or("stash@{0}");
        if is_valid_git_oid(target) {
            return Ok(Some(target.to_string()));
        }
        if matches!(target, "stash" | "refs/stash") {
            return self.resolve_stash_target_at_cursor(Some(&"stash@{0}".to_string()));
        }
        let Some(index) = target
            .strip_prefix("stash@{")
            .and_then(|value| value.strip_suffix('}'))
            .and_then(|value| value.parse::<usize>().ok())
        else {
            return Ok(None);
        };
        if let Some(oid) = self.stash_stack.get(index) {
            return Ok(Some(oid.clone()));
        }
        let path = self.common_dir().join("logs").join("refs/stash");
        let key = common_key("refs/stash");
        let entries = read_reflog_entries(key.clone(), &path, "refs/stash", Some(0))?;
        let cursor = self.offsets.get(&key).copied().unwrap_or(u64::MAX);
        let mut stack = entries
            .into_iter()
            .filter(|entry| entry.end_offset <= cursor)
            .filter(|entry| valid_non_zero_oid(&entry.new))
            .map(|entry| entry.new)
            .collect::<Vec<_>>();
        stack.reverse();
        Ok(stack.get(index).cloned())
    }

    pub(super) fn apply_stash_ref_entry(&mut self, kind: &str, entry: &CursorEntry) {
        match kind {
            "push" | "save" => {
                if valid_non_zero_oid(&entry.new)
                    && !self.stash_stack.iter().any(|oid| oid == &entry.new)
                {
                    self.stash_stack.insert(0, entry.new.clone());
                }
            }
            "pop" | "drop" | "branch" => {
                if let Some(position) = self.stash_stack.iter().position(|oid| oid == &entry.old) {
                    self.stash_stack.remove(position);
                }
                if valid_non_zero_oid(&entry.new)
                    && !self.stash_stack.iter().any(|oid| oid == &entry.new)
                {
                    self.stash_stack.insert(0, entry.new.clone());
                }
            }
            _ => {}
        }
    }

    pub(super) fn sync_common_ref_cursor_to_log_end_after_rewrite(
        &mut self,
        reference: &str,
    ) -> Result<(), GitAiError> {
        let key = common_key(reference);
        let path = self.common_dir().join("logs").join(reference);
        match fs::metadata(&path) {
            Ok(metadata) => {
                let len = metadata.len();
                self.offsets.insert(key.clone(), len);
                self.consumed_offsets.remove(&key);
                self.consumed_anchors.remove(&key);
                if let Some(record) = read_reflog_record_ending_at(&path, len)? {
                    self.anchors.insert(key, ReflogAnchor::from(&record));
                } else {
                    self.anchors.remove(&key);
                }
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.clear_ref_cursor(&key);
                Ok(())
            }
            Err(error) => Err(GitAiError::IoError(error)),
        }
    }

    pub(super) fn common_ref_log_len(&self, reference: &str) -> Result<Option<u64>, GitAiError> {
        let path = self.common_dir().join("logs").join(reference);
        match fs::metadata(path) {
            Ok(metadata) => Ok(Some(metadata.len())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(GitAiError::IoError(error)),
        }
    }

    pub(super) fn remove_stash_from_stack(
        &mut self,
        target_index: Option<usize>,
        target_oid: &str,
    ) {
        if let Some(index) = target_index
            && self
                .stash_stack
                .get(index)
                .is_some_and(|oid| oid == target_oid)
        {
            self.stash_stack.remove(index);
            return;
        }
        if let Some(position) = self.stash_stack.iter().position(|oid| oid == target_oid) {
            self.stash_stack.remove(position);
        }
    }
}

pub(super) fn parse_update_ref_spec(args: &[String]) -> Result<Option<UpdateRefSpec>, GitAiError> {
    let mut positionals = Vec::new();
    let mut delete = false;
    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "update-ref" => {
                idx += 1;
            }
            "--stdin" | "--batch-updates" => {
                return Ok(None);
            }
            "-d" | "--delete" => {
                delete = true;
                idx += 1;
            }
            "-m" | "--message" => {
                if idx + 1 >= args.len() {
                    return Err(GitAiError::Generic(
                        "update-ref -m requires a message argument".to_string(),
                    ));
                }
                idx += 2;
            }
            "--create-reflog" | "--no-deref" => {
                idx += 1;
            }
            value if value.starts_with("--message=") => {
                idx += 1;
            }
            value if value.starts_with('-') => {
                return Err(GitAiError::Generic(format!(
                    "trace2 cursor does not support update-ref option '{}'",
                    value
                )));
            }
            value => {
                positionals.push(value.to_string());
                idx += 1;
            }
        }
    }

    if delete {
        return match positionals.as_slice() {
            [reference] => Ok(Some(UpdateRefSpec {
                reference: reference.to_string(),
                new_oid: zero_oid(),
                old_oid: None,
            })),
            [reference, old_oid] => Ok(Some(UpdateRefSpec {
                reference: reference.to_string(),
                new_oid: zero_oid(),
                old_oid: Some(old_oid.to_string()),
            })),
            _ => Err(GitAiError::Generic(
                "update-ref delete requires <ref> [<old-oid>]".to_string(),
            )),
        };
    }

    match positionals.as_slice() {
        [reference, new_oid] => Ok(Some(UpdateRefSpec {
            reference: reference.to_string(),
            new_oid: new_oid.to_string(),
            old_oid: None,
        })),
        [reference, new_oid, old_oid] => Ok(Some(UpdateRefSpec {
            reference: reference.to_string(),
            new_oid: new_oid.to_string(),
            old_oid: Some(old_oid.to_string()),
        })),
        _ => Err(GitAiError::Generic(
            "update-ref requires <ref> <new-oid> [<old-oid>]".to_string(),
        )),
    }
}

pub(super) fn direct_update_ref_change_from_argv(spec: &UpdateRefSpec) -> Option<RefChange> {
    let old = spec.old_oid.as_ref()?;
    if !is_valid_git_oid(old) || !is_valid_git_oid(&spec.new_oid) {
        return None;
    }
    Some(RefChange {
        reference: spec.reference.clone(),
        old: old.clone(),
        new: spec.new_oid.clone(),
    })
}

pub(super) fn stash_command_args(args: &[String]) -> &[String] {
    if args.first().is_some_and(|arg| arg == "stash") {
        &args[1..]
    } else {
        args
    }
}

pub(super) fn stash_push_message_from_args(args: &[String], kind: &str) -> Option<String> {
    if kind == "save" {
        let message = args
            .iter()
            .skip(1)
            .filter(|arg| !arg.starts_with('-'))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        return (!message.is_empty()).then_some(message);
    }

    let mut idx = if kind == "push" { 1 } else { 0 };
    while idx < args.len() {
        match args[idx].as_str() {
            "-m" | "--message" => {
                return args.get(idx + 1).filter(|value| !value.is_empty()).cloned();
            }
            value if value.starts_with("--message=") => {
                return value
                    .strip_prefix("--message=")
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned);
            }
            _ => idx += 1,
        }
    }
    None
}

pub(super) fn stash_reflog_message_matches(reflog_message: &str, stash_message: &str) -> bool {
    reflog_message == stash_message
        || reflog_message
            .strip_suffix(stash_message)
            .is_some_and(|prefix| prefix.ends_with(": "))
}

pub(super) fn stash_target_index(target: Option<&String>) -> Option<usize> {
    let target = target.map(String::as_str).unwrap_or("stash@{0}");
    if matches!(target, "stash" | "refs/stash") {
        return Some(0);
    }
    target
        .strip_prefix("stash@{")
        .and_then(|value| value.strip_suffix('}'))
        .and_then(|value| value.parse::<usize>().ok())
}
