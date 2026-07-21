use super::*;

impl RefCursor {
    pub(super) fn enrich_branch(
        &mut self,
        cmd: &mut NormalizedCommand,
        state: &FamilyState,
    ) -> Result<(), GitAiError> {
        let args = command_args(cmd);
        let spec = parse_branch_command_spec(&args);
        let mut changes = Vec::new();

        match spec {
            BranchCommandSpec::CreateOrReset { reference } => {
                if let Some(entry) = self.find_common_ref_entry(
                    &reference,
                    ExpectedTransition::default(),
                    &["branch:"],
                )? {
                    self.consume_entry(&entry)?;
                    changes.push(entry_to_ref_change(&entry));
                }
            }
            BranchCommandSpec::Delete { references } => {
                let zero = zero_oid();
                for reference in references {
                    self.clear_ref_cursor(&common_key(&reference));
                    if let Some(old) = state
                        .refs
                        .get(&reference)
                        .filter(|oid| valid_non_zero_oid(oid))
                    {
                        changes.push(RefChange {
                            reference,
                            old: old.clone(),
                            new: zero.clone(),
                        });
                    }
                }
            }
            BranchCommandSpec::Rename {
                old_reference,
                new_reference,
            } => {
                self.enrich_branch_relocation(
                    state,
                    BranchLifecycleKind::Rename,
                    old_reference,
                    new_reference,
                    &mut changes,
                )?;
            }
            BranchCommandSpec::Copy {
                old_reference,
                new_reference,
            } => {
                self.enrich_branch_relocation(
                    state,
                    BranchLifecycleKind::Copy,
                    old_reference,
                    new_reference,
                    &mut changes,
                )?;
            }
            BranchCommandSpec::None => {}
        }

        dedup_ref_changes(&mut changes);
        cmd.ref_changes = changes;
        Ok(())
    }

    pub(super) fn enrich_branch_relocation(
        &mut self,
        state: &FamilyState,
        kind: BranchLifecycleKind,
        old_reference: Option<String>,
        new_reference: String,
        changes: &mut Vec<RefChange>,
    ) -> Result<(), GitAiError> {
        let lifecycle = self.consume_branch_lifecycle_record(&new_reference, kind)?;
        let source_reference = old_reference.or_else(|| {
            lifecycle
                .as_ref()
                .map(|record| record.old_reference.clone())
        });
        let source_oid = source_reference
            .as_ref()
            .and_then(|reference| state.refs.get(reference).cloned())
            .or_else(|| lifecycle.as_ref().map(|record| record.oid.clone()));
        let Some(source_oid) = source_oid.filter(|oid| valid_non_zero_oid(oid)) else {
            return Ok(());
        };

        if kind == BranchLifecycleKind::Rename
            && let Some(source_reference) = source_reference.as_ref()
            && source_reference != &new_reference
        {
            self.clear_ref_cursor(&common_key(source_reference));
            changes.push(RefChange {
                reference: source_reference.clone(),
                old: source_oid.clone(),
                new: zero_oid(),
            });
        }

        let new_old = state
            .refs
            .get(&new_reference)
            .filter(|oid| valid_non_zero_oid(oid))
            .cloned()
            .unwrap_or_else(zero_oid);
        if new_old != source_oid {
            changes.push(RefChange {
                reference: new_reference.clone(),
                old: new_old,
                new: source_oid,
            });
        }
        Ok(())
    }

    pub(super) fn consume_branch_lifecycle_record(
        &mut self,
        reference: &str,
        kind: BranchLifecycleKind,
    ) -> Result<Option<BranchLifecycleRecord>, GitAiError> {
        let path = self.common_dir().join("logs").join(reference);
        let key = common_key(reference);
        let start = self.reflog_start_offset(&key, &path)?;
        let entries = read_reflog_entries(key.clone(), &path, reference, start)?;
        for entry in entries {
            let Some((old_reference, new_reference)) =
                parse_branch_lifecycle_message(kind, &entry.message)
            else {
                continue;
            };
            if new_reference != reference {
                continue;
            }
            self.consume_entry(&entry)?;
            return Ok(Some(BranchLifecycleRecord {
                old_reference,
                oid: entry.new,
            }));
        }
        Ok(None)
    }
}

pub(super) fn parse_branch_command_spec(args: &[String]) -> BranchCommandSpec {
    let args = branch_command_args(args);
    let mut delete = false;
    let mut remote_delete = false;
    let mut rename = false;
    let mut copy = false;
    let mut list_only = false;
    let mut config_only = false;
    let mut positionals = Vec::new();
    let mut idx = 0usize;

    while idx < args.len() {
        let arg = &args[idx];
        if arg == "--" {
            positionals.extend(args[idx + 1..].iter().cloned());
            break;
        }

        match arg.as_str() {
            "-d" | "-D" | "--delete" => {
                delete = true;
                idx += 1;
            }
            "-m" | "-M" | "--move" => {
                rename = true;
                idx += 1;
            }
            "-c" | "-C" | "--copy" => {
                copy = true;
                idx += 1;
            }
            "-r" | "--remotes" => {
                remote_delete = true;
                list_only = true;
                idx += 1;
            }
            "-a" | "--all" | "--list" | "--show-current" | "--contains" | "--no-contains"
            | "--merged" | "--no-merged" => {
                list_only = true;
                idx += 1;
            }
            "--unset-upstream" | "--edit-description" | "--set-upstream" => {
                config_only = true;
                idx += 1;
            }
            "-u" | "--set-upstream-to" => {
                config_only = true;
                idx = idx.saturating_add(2);
            }
            "--points-at" | "--sort" | "--format" => {
                list_only = true;
                idx = idx.saturating_add(2);
            }
            "--color" | "--column" | "--abbrev" => {
                idx = idx.saturating_add(2);
            }
            "--track"
            | "--no-track"
            | "--create-reflog"
            | "--no-create-reflog"
            | "--recurse-submodules"
            | "--no-color"
            | "--no-column"
            | "--no-abbrev"
            | "--quiet"
            | "-q"
            | "--verbose"
            | "-v"
            | "-vv"
            | "-f"
            | "--force"
            | "-l" => {
                idx += 1;
            }
            value if value.starts_with("--set-upstream-to=") => {
                config_only = true;
                idx += 1;
            }
            value
                if value.starts_with("--points-at=")
                    || value.starts_with("--sort=")
                    || value.starts_with("--format=")
                    || value.starts_with("--contains=")
                    || value.starts_with("--no-contains=")
                    || value.starts_with("--merged=")
                    || value.starts_with("--no-merged=") =>
            {
                list_only = true;
                idx += 1;
            }
            value
                if value.starts_with("--track=")
                    || value.starts_with("--color=")
                    || value.starts_with("--column=")
                    || value.starts_with("--abbrev=") =>
            {
                idx += 1;
            }
            value if value.starts_with("--") => {
                idx += 1;
            }
            value if value.starts_with('-') => {
                apply_branch_short_options(
                    value,
                    &mut delete,
                    &mut remote_delete,
                    &mut rename,
                    &mut copy,
                    &mut list_only,
                );
                idx += branch_short_option_value_width(value);
            }
            value => {
                positionals.push(value.to_string());
                idx += 1;
            }
        }
    }

    if delete {
        let references = positionals
            .into_iter()
            .filter_map(|name| branch_ref_name(&name, remote_delete))
            .collect::<Vec<_>>();
        return if references.is_empty() {
            BranchCommandSpec::None
        } else {
            BranchCommandSpec::Delete { references }
        };
    }

    if rename {
        return match positionals.as_slice() {
            [new_name] => branch_ref_name(new_name, false)
                .map(|new_reference| BranchCommandSpec::Rename {
                    old_reference: None,
                    new_reference,
                })
                .unwrap_or(BranchCommandSpec::None),
            [old_name, new_name] => {
                match (
                    branch_ref_name(old_name, false),
                    branch_ref_name(new_name, false),
                ) {
                    (Some(old_reference), Some(new_reference)) => BranchCommandSpec::Rename {
                        old_reference: Some(old_reference),
                        new_reference,
                    },
                    _ => BranchCommandSpec::None,
                }
            }
            _ => BranchCommandSpec::None,
        };
    }

    if copy {
        return match positionals.as_slice() {
            [new_name] => branch_ref_name(new_name, false)
                .map(|new_reference| BranchCommandSpec::Copy {
                    old_reference: None,
                    new_reference,
                })
                .unwrap_or(BranchCommandSpec::None),
            [old_name, new_name] => {
                match (
                    branch_ref_name(old_name, false),
                    branch_ref_name(new_name, false),
                ) {
                    (Some(old_reference), Some(new_reference)) => BranchCommandSpec::Copy {
                        old_reference: Some(old_reference),
                        new_reference,
                    },
                    _ => BranchCommandSpec::None,
                }
            }
            _ => BranchCommandSpec::None,
        };
    }

    if config_only || list_only {
        return BranchCommandSpec::None;
    }

    positionals
        .first()
        .and_then(|name| branch_ref_name(name, false))
        .map(|reference| BranchCommandSpec::CreateOrReset { reference })
        .unwrap_or(BranchCommandSpec::None)
}

pub(super) fn branch_command_args(args: &[String]) -> &[String] {
    if args.first().is_some_and(|arg| arg == "branch") {
        &args[1..]
    } else {
        args
    }
}

pub(super) fn apply_branch_short_options(
    value: &str,
    delete: &mut bool,
    remote_delete: &mut bool,
    rename: &mut bool,
    copy: &mut bool,
    list_only: &mut bool,
) {
    for flag in value.trim_start_matches('-').chars() {
        match flag {
            'd' | 'D' => *delete = true,
            'r' => {
                *remote_delete = true;
                *list_only = true;
            }
            'm' | 'M' => *rename = true,
            'c' | 'C' => *copy = true,
            'a' => *list_only = true,
            _ => {}
        }
    }
}

pub(super) fn branch_short_option_value_width(value: &str) -> usize {
    if value == "-u" { 2 } else { 1 }
}

pub(super) fn branch_ref_name(name: &str, remote: bool) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "--" || trimmed.starts_with('-') {
        return None;
    }
    if trimmed.starts_with("refs/heads/") || trimmed.starts_with("refs/remotes/") {
        return Some(trimmed.to_string());
    }
    if trimmed.starts_with("refs/") {
        return None;
    }
    if remote {
        Some(format!("refs/remotes/{}", trimmed))
    } else {
        Some(format!("refs/heads/{}", trimmed))
    }
}

pub(super) fn parse_branch_lifecycle_message(
    kind: BranchLifecycleKind,
    message: &str,
) -> Option<(String, String)> {
    let prefix = match kind {
        BranchLifecycleKind::Rename => "Branch: renamed ",
        BranchLifecycleKind::Copy => "Branch: copied ",
    };
    let rest = message.strip_prefix(prefix)?;
    let (old_reference, new_reference) = rest.split_once(" to ")?;
    Some((old_reference.to_string(), new_reference.to_string()))
}
