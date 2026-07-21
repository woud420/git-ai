use super::carryover_merge::diff_hunks_between_contents;
use super::log_split_stages::{
    append_committed_attestations, collect_commit_and_workdir_hunks, extend_pathspecs_for_renames,
    fill_committed_gaps, filter_committed_overlap_from_unstaged, resolve_rename_map,
};
use super::types::{AuthorshipLogDiffContext, VirtualAttributions};
use crate::error::GitAiError;
use crate::model::attribution_tracker::LineAttribution;
use crate::model::hunk_shift::apply_hunk_shifts_to_line_attributions;
use crate::model::working_log::CheckpointKind;
use std::collections::{BTreeMap, HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

impl VirtualAttributions {
    /// As [`Self::to_authorship_log_and_initial_working_log`], but accepts a
    /// pre-computed parent→commit `DiffTreeResult` so a batched caller (e.g. the
    /// rebase conflict-resolution driver) can supply renames + committed hunks
    /// from a single batched `diff-tree` instead of this method spawning its own
    /// per-commit `git diff` / `git diff-tree`.
    ///
    /// When the context has no precomputed diff, it may override the diff base
    /// used only for committed-hunk and rename classification. Other
    /// reconciliation still uses `parent_sha`.
    pub(crate) fn to_authorship_log_and_initial_working_log_with_precomputed_diff(
        &self,
        repo: &crate::operations::git::repository::Repository,
        parent_sha: &str,
        commit_sha: &str,
        pathspecs: Option<&HashSet<String>>,
        final_state_snapshot: Option<&HashMap<String, String>>,
        diff_context: AuthorshipLogDiffContext<'_>,
    ) -> Result<
        (
            crate::model::authorship_log_serialization::AuthorshipLog,
            crate::operations::git::repo_storage::InitialAttributions,
            HashMap<String, String>,
        ),
        GitAiError,
    > {
        use crate::model::authorship_log::{HumanRecord, SessionRecord};
        use crate::operations::git::repo_storage::InitialAttributions;

        let mut authorship_log = self.seed_authorship_log_metadata();

        let mut initial_files: HashMap<String, Vec<LineAttribution>> = HashMap::new();
        let mut referenced_prompts: HashSet<String> = HashSet::new();
        let mut initial_humans: BTreeMap<String, HumanRecord> = BTreeMap::new();
        let mut initial_sessions: BTreeMap<String, SessionRecord> = BTreeMap::new();
        let mut initial_file_contents: HashMap<String, String> = HashMap::new();

        // Detect renames so we can look up committed hunks by new path when
        // the working log references the old path. A batched caller may supply
        // the parent→commit diff (renames included); otherwise spawn per-commit.
        //
        // The persisted working-log base can be an older ref tip on daemon
        // fast-forward paths. Diff classification must still be bounded to the
        // finalized commit, while carryover reconciliation below continues to
        // use the original working-log base.
        let precomputed_parent_diff = diff_context.precomputed_parent_diff;
        let fallback_diff_base = diff_context
            .fallback_committed_diff_base
            .unwrap_or(parent_sha);
        let rename_map = resolve_rename_map(
            repo,
            parent_sha,
            fallback_diff_base,
            precomputed_parent_diff,
            commit_sha,
        );

        // Extend pathspecs with renamed-to paths so diff_added_lines doesn't filter them out.
        let extended_pathspecs = extend_pathspecs_for_renames(pathspecs, &rename_map);
        let effective_pathspecs = extended_pathspecs.as_deref();

        // Collect committed hunks (commit coordinates) and unstaged hunks (workdir coordinates).
        // All git spawns happen here, before the per-file loop.
        let mut hunks = collect_commit_and_workdir_hunks(
            repo,
            parent_sha,
            fallback_diff_base,
            commit_sha,
            effective_pathspecs,
            final_state_snapshot,
            precomputed_parent_diff,
        )?;

        // Drop unstaged line numbers that are also committed; keep pure insertions.
        filter_committed_overlap_from_unstaged(
            &mut hunks.unstaged,
            &hunks.committed,
            &hunks.pure_insertions,
        );

        // Process each file
        for (file_path, (_, line_attrs)) in &self.attributions {
            if line_attrs.is_empty() {
                continue;
            }

            // Diff output keys are NFC-normalised, but working-log paths may be
            // NFD.  Compute the NFC form once for all lookups in this iteration.
            let nfc_file_path: String = file_path.nfc().collect();

            // Rebase line attributions against the carryover snapshot when present.
            // The shadow keeps the zero-copy path (no snapshot) allocation-free.
            let rebased_line_attrs;
            let line_attrs: &[LineAttribution] = if let Some(snapshot) = &hunks.carryover_snapshot {
                // Snapshot lookup: NFC-then-raw (diff keys are NFC).
                let carryover_content = snapshot
                    .get(&nfc_file_path)
                    .or_else(|| snapshot.get(file_path))
                    .ok_or_else(|| {
                        GitAiError::Generic(format!(
                            "carryover snapshot missing content for {}",
                            file_path
                        ))
                    })?;
                // self.file_contents lookup: raw-then-NFC (working-log paths are raw).
                let observed_content = self
                    .file_contents
                    .get(file_path)
                    .or_else(|| self.file_contents.get(&nfc_file_path))
                    .ok_or_else(|| {
                        GitAiError::Generic(format!(
                            "virtual attribution missing content for {}",
                            file_path
                        ))
                    })?;
                let shift_hunks = diff_hunks_between_contents(observed_content, carryover_content);
                rebased_line_attrs =
                    apply_hunk_shifts_to_line_attributions(line_attrs, &shift_hunks);
                &rebased_line_attrs
            } else {
                line_attrs
            };

            // Expand this file's unstaged ranges to a sorted Vec for binary_search.
            let mut unstaged_lines: Vec<u32> = Vec::new();
            let unstaged_lookup = hunks.unstaged.get(&nfc_file_path).or_else(|| {
                rename_map
                    .get(&nfc_file_path)
                    .and_then(|np| hunks.unstaged.get(np))
            });
            if let Some(unstaged_ranges) = unstaged_lookup {
                for range in unstaged_ranges {
                    unstaged_lines.extend(range.expand());
                }
                unstaged_lines.sort_unstable();
            }

            // Committed hunks for this file are keyed by new path after renames.
            let file_committed_hunks = hunks.committed.get(&nfc_file_path).or_else(|| {
                rename_map
                    .get(&nfc_file_path)
                    .and_then(|np| hunks.committed.get(np))
            });

            // Classify each workdir line as committed or uncommitted.
            let mut committed_lines_map: HashMap<String, Vec<u32>> = HashMap::new();
            let mut uncommitted_lines_map: HashMap<String, Vec<u32>> = HashMap::new();
            let is_renamed_file = rename_map.contains_key(&nfc_file_path);

            for line_attr in line_attrs {
                for workdir_line_num in line_attr.start_line..=line_attr.end_line {
                    let is_unstaged = unstaged_lines.binary_search(&workdir_line_num).is_ok();

                    if is_unstaged {
                        uncommitted_lines_map
                            .entry(line_attr.author_id.clone())
                            .or_default()
                            .push(workdir_line_num);
                        referenced_prompts.insert(line_attr.author_id.clone());
                    } else {
                        // Convert workdir → commit line number by subtracting
                        // the count of unstaged lines that precede this one.
                        let adjustment = unstaged_lines
                            .iter()
                            .filter(|&&l| l < workdir_line_num)
                            .count() as u32;
                        let commit_line_num = workdir_line_num - adjustment;

                        let is_committed = file_committed_hunks
                            .map(|hunks| hunks.iter().any(|h| h.contains(commit_line_num)))
                            .unwrap_or(false);

                        if is_committed {
                            committed_lines_map
                                .entry(line_attr.author_id.clone())
                                .or_default()
                                .push(commit_line_num);
                        } else if is_renamed_file
                            && line_attr.author_id != CheckpointKind::Human.to_str()
                            && !line_attr.author_id.starts_with("h_")
                        {
                            // For renamed files, git blame attributes ALL lines to
                            // this commit.  Include AI lines even outside committed
                            // hunks so they have an attestation entry.
                            committed_lines_map
                                .entry(line_attr.author_id.clone())
                                .or_default()
                                .push(commit_line_num);
                        }
                    }
                }
            }

            // Fill imara_diff Equal-match gaps in committed hunks.
            if let Some(hunks_for_file) = file_committed_hunks {
                let file_content = self
                    .file_contents
                    .get(file_path)
                    .or_else(|| self.file_contents.get(&nfc_file_path))
                    .map(String::as_str);
                fill_committed_gaps(&mut committed_lines_map, hunks_for_file, file_content);
            }

            // Emit attestation entries.  Attestation path uses NFC (diff-key space).
            if !committed_lines_map.is_empty() {
                let attestation_path = rename_map.get(&nfc_file_path).unwrap_or(&nfc_file_path);
                append_committed_attestations(
                    &mut authorship_log,
                    committed_lines_map,
                    attestation_path,
                );
            }

            // Emit INITIAL uncommitted attrs.  Initial path uses raw working-log path.
            if !uncommitted_lines_map.is_empty() {
                let uncommitted_attrs = self.build_uncommitted_initial_attrs(
                    uncommitted_lines_map,
                    &mut initial_humans,
                    &mut initial_sessions,
                );
                let initial_path = rename_map.get(file_path).unwrap_or(file_path);
                initial_files.insert(initial_path.clone(), uncommitted_attrs);
                self.record_initial_file(
                    initial_path,
                    file_path,
                    &nfc_file_path,
                    hunks.carryover_snapshot.as_ref(),
                    &mut initial_file_contents,
                );
            }
        }

        self.prune_unreferenced_metadata(&mut authorship_log);

        let initial_prompts = self.build_initial_prompts(referenced_prompts);

        let initial_attributions = InitialAttributions {
            files: initial_files,
            prompts: initial_prompts,
            file_blobs: HashMap::new(),
            humans: initial_humans,
            sessions: initial_sessions,
        };

        Ok((authorship_log, initial_attributions, initial_file_contents))
    }
}
