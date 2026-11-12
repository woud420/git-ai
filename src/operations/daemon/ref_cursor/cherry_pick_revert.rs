use super::*;

impl RefCursor {
    /// Sources carried over from a conflicted cherry-pick. An enrichment pass
    /// consumes them even when it finds no reflog row, so a caller that
    /// enriches the same command again must restore them first.
    pub(crate) fn pending_cherry_pick_sources(&self) -> HashMap<String, Vec<String>> {
        self.pending_cherry_pick_source_oids.clone()
    }

    pub(crate) fn restore_pending_cherry_pick_sources(
        &mut self,
        sources: &HashMap<String, Vec<String>>,
    ) {
        self.pending_cherry_pick_source_oids = sources.clone();
    }

    pub(super) fn pending_cherry_pick_sources_for(&self, cmd: &NormalizedCommand) -> &[String] {
        self.pending_cherry_pick_source_oids
            .get(&pending_cherry_pick_key(cmd))
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn enrich_cherry_pick(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        let args = parsed_invocation_for_normalized_command(cmd).command_args;
        let key = pending_cherry_pick_key(cmd);
        if args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--abort" | "--quit"))
        {
            self.pending_cherry_pick_source_oids.remove(&key);
            return Ok(());
        }

        let is_no_commit = args.iter().any(|arg| arg == "--no-commit" || arg == "-n");
        let is_continue = args.iter().any(|arg| arg == "--continue");
        let is_skip = args.iter().any(|arg| arg == "--skip");

        if is_skip
            && let Some(pending) = self.pending_cherry_pick_source_oids.get_mut(&key)
            && !pending.is_empty()
        {
            pending.remove(0);
        }

        let source_args = if is_continue || is_skip {
            Vec::new()
        } else {
            cherry_pick_source_args_from_command_args(&args)
        };
        let explicit_sources = if is_continue || is_skip {
            Some(Vec::new())
        } else {
            resolve_cherry_pick_source_oids_from_sources(cmd, state, &source_args)?
        };
        let unresolved_explicit_sources = !source_args.is_empty() && explicit_sources.is_none();
        let explicit_sources = explicit_sources.unwrap_or_default();
        cmd.cherry_pick_source_oids = if explicit_sources.is_empty() && !unresolved_explicit_sources
        {
            self.pending_cherry_pick_source_oids
                .get(&key)
                .cloned()
                .unwrap_or_default()
        } else {
            explicit_sources
        };

        if cmd.exit_code != 0 && unresolved_explicit_sources {
            return Ok(());
        }

        if is_no_commit {
            return Ok(());
        }

        let source_limit = if unresolved_explicit_sources {
            usize::MAX
        } else {
            cmd.cherry_pick_source_oids.len().max(1)
        };
        self.consume_head_span_for_command_limited(
            cmd,
            state,
            CHERRY_PICK_REFLOG_PREFIXES,
            self.head_expected_transition(cmd, state),
            source_limit,
        )?;

        let applied_count = cmd
            .ref_changes
            .iter()
            .filter(|change| change.reference == "HEAD")
            .count();
        if cmd.exit_code != 0 {
            let remaining = cmd
                .cherry_pick_source_oids
                .iter()
                .skip(applied_count.min(cmd.cherry_pick_source_oids.len()))
                .cloned()
                .collect();
            self.pending_cherry_pick_source_oids.insert(key, remaining);
        } else if is_continue
            || is_skip
            || !cmd.cherry_pick_source_oids.is_empty()
            || applied_count > 0
        {
            self.pending_cherry_pick_source_oids.remove(&key);
        }

        Ok(())
    }

    pub(super) fn enrich_revert(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        let args = parsed_invocation_for_normalized_command(cmd).command_args;
        if args
            .iter()
            .any(|arg| matches!(arg.as_str(), "--abort" | "--quit"))
        {
            return Ok(());
        }

        let is_no_commit = args.iter().any(|arg| arg == "--no-commit" || arg == "-n");
        let is_continue = args.iter().any(|arg| arg == "--continue");
        let is_skip = args.iter().any(|arg| arg == "--skip");
        // DEFERRED: on `revert --continue` (resuming after a
        // conflict) the original source OIDs are not on the command line and we
        // carry no `pending_revert_source_oids` from the interrupted revert, so
        // revert_source_oids ends up empty. handle_revert_commit then falls back
        // to first-parent, which is only exact for `git revert HEAD`; a
        // multi-commit `revert A B` resumed via --continue can reconstruct the
        // wrong source base. A precise fix needs the daemon to persist the
        // pending source OIDs across the conflict pause and replay them here.
        let source_args = if is_continue || is_skip {
            Vec::new()
        } else {
            revert_source_args_from_command_args(&args)
        };
        let explicit_sources = if source_args.is_empty() {
            Some(Vec::new())
        } else {
            resolve_cherry_pick_source_oids_from_sources(cmd, state, &source_args)?
        };
        let unresolved_explicit_sources = !source_args.is_empty() && explicit_sources.is_none();
        cmd.revert_source_oids = explicit_sources.unwrap_or_default();

        if is_no_commit {
            return Ok(());
        }

        let source_limit = if unresolved_explicit_sources {
            usize::MAX
        } else {
            cmd.revert_source_oids.len().max(1)
        };
        self.consume_head_span_for_command_limited(
            cmd,
            state,
            &["revert:"],
            self.head_expected_transition(cmd, state),
            source_limit,
        )
    }
}

fn pending_cherry_pick_key(cmd: &NormalizedCommand) -> String {
    cmd.worktree
        .as_deref()
        .map(crate::operations::daemon::ActorDaemonCoordinator::worktree_state_key)
        .unwrap_or_default()
}
