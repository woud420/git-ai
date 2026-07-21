use super::attestation::committed_hunks_from_diff_result;
use super::carryover_merge::diff_hunks_between_contents;
use super::carryover_snapshot::build_carryover_snapshot;
use super::diff_utils::{
    collect_committed_hunks, collect_unstaged_hunks, collect_unstaged_hunks_from_snapshot,
    detect_renames_in_commit,
};
use super::types::{AuthorshipLogDiffContext, VirtualAttributions};
use crate::error::GitAiError;
use crate::model::authorship_log::{HumanRecord, LineRange, SessionRecord};
use crate::model::working_log::CheckpointKind;
use crate::operations::authorship::attribution_tracker::LineAttribution;
use crate::operations::authorship::hunk_shift::apply_hunk_shifts_to_line_attributions;
use crate::operations::git::repository::Repository;
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
        repo: &Repository,
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
        use crate::model::authorship_log_serialization::AuthorshipLog;
        use crate::operations::git::repo_storage::InitialAttributions;
        use std::collections::{HashMap as StdHashMap, HashSet};

        let mut authorship_log = AuthorshipLog::new();
        authorship_log.metadata.base_commit_sha = self.base_commit.clone();
        // Flatten the nested prompts map: take the most recent (first) prompt for each prompt_id
        authorship_log.metadata.prompts = self
            .prompts
            .iter()
            .filter_map(|(prompt_id, commits)| {
                // Get the first (most recent) commit's PromptRecord
                commits
                    .values()
                    .next()
                    .map(|record| (prompt_id.clone(), record.clone()))
            })
            .collect();
        authorship_log.metadata.humans = self.humans.clone();
        authorship_log.metadata.sessions = self.sessions.clone();

        let mut initial_files: StdHashMap<String, Vec<LineAttribution>> = StdHashMap::new();
        let mut referenced_prompts: HashSet<String> = HashSet::new();
        let mut initial_humans: BTreeMap<String, HumanRecord> = BTreeMap::new();
        let mut initial_sessions: BTreeMap<String, SessionRecord> = BTreeMap::new();
        let mut initial_file_contents: StdHashMap<String, String> = StdHashMap::new();

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
        let rename_map = if let Some(diff) = precomputed_parent_diff {
            diff.renames.iter().cloned().collect()
        } else if parent_sha != "initial" {
            detect_renames_in_commit(repo, fallback_diff_base, commit_sha).unwrap_or_default()
        } else {
            HashMap::new()
        };

        // Extend pathspecs with renamed-to paths so diff_added_lines doesn't filter them out.
        let extended_pathspecs;
        let effective_pathspecs = if !rename_map.is_empty()
            && let Some(ps_ref) = pathspecs
        {
            let mut ps = ps_ref.clone();
            for (old_path, new_path) in &rename_map {
                if ps.contains(old_path) {
                    ps.insert(new_path.clone());
                }
            }
            extended_pathspecs = ps;
            Some(&extended_pathspecs)
        } else {
            pathspecs
        };

        // Get committed hunks (in commit coordinates) and unstaged hunks (in working directory coordinates)
        let committed_hunks = if let Some(diff) = precomputed_parent_diff {
            committed_hunks_from_diff_result(diff, effective_pathspecs)
        } else {
            collect_committed_hunks(repo, fallback_diff_base, commit_sha, effective_pathspecs)?
        };
        let carryover_snapshot = if let Some(snapshot) = final_state_snapshot {
            Some(build_carryover_snapshot(
                repo,
                parent_sha,
                commit_sha,
                effective_pathspecs,
                snapshot,
            )?)
        } else {
            None
        };
        let (mut unstaged_hunks, pure_insertion_hunks) = if let Some(snapshot) = &carryover_snapshot
        {
            collect_unstaged_hunks_from_snapshot(repo, commit_sha, effective_pathspecs, snapshot)?
        } else {
            collect_unstaged_hunks(repo, commit_sha, effective_pathspecs)?
        };

        // IMPORTANT: If a line appears in both committed_hunks and unstaged_hunks, it means:
        // - The line was committed in this commit (in commit coordinates)
        // - The line was then modified again in the working directory (in workdir coordinates)
        // Since both use the same line numbering after the commit (workdir coordinates = commit coordinates
        // for the committed state), we can directly compare line numbers.
        // We should treat these lines as committed, not unstaged, because the attribution belongs
        // to the commit even if there's a subsequent unstaged modification.
        //
        // HOWEVER: If a line is a PURE INSERTION (old_count=0), it means a new line was inserted
        // at that position, pushing existing lines down. In this case, the line number overlap
        // doesn't mean the same line - it's a different line at the same position!
        // We should NOT filter out pure insertions even if they overlap with committed line numbers.
        for (file_path, committed_ranges) in &committed_hunks {
            if let Some(unstaged_ranges) = unstaged_hunks.get_mut(file_path) {
                // Expand both to line numbers for comparison
                let committed_lines: std::collections::HashSet<u32> =
                    committed_ranges.iter().flat_map(|r| r.expand()).collect();

                // Get pure insertion lines for this file (these should NOT be filtered out)
                let pure_insertion_lines: std::collections::HashSet<u32> = pure_insertion_hunks
                    .get(file_path)
                    .map(|ranges| ranges.iter().flat_map(|r| r.expand()).collect())
                    .unwrap_or_default();

                // Filter out any unstaged lines that were also committed
                // (these are lines that were committed, then modified again in workdir)
                // BUT keep pure insertions even if they overlap with committed line numbers
                let mut filtered_unstaged_lines: Vec<u32> = unstaged_ranges
                    .iter()
                    .flat_map(|r| r.expand())
                    .filter(|line| {
                        // Keep the line if it's NOT in committed, OR if it's a pure insertion
                        !committed_lines.contains(line) || pure_insertion_lines.contains(line)
                    })
                    .collect();

                if filtered_unstaged_lines.is_empty() {
                    unstaged_ranges.clear();
                } else {
                    filtered_unstaged_lines.sort_unstable();
                    filtered_unstaged_lines.dedup();
                    *unstaged_ranges = LineRange::compress_lines(&filtered_unstaged_lines);
                }
            }
        }

        // Remove files with no unstaged hunks
        unstaged_hunks.retain(|_, ranges| !ranges.is_empty());

        // Process each file
        for (file_path, (_, line_attrs)) in &self.attributions {
            if line_attrs.is_empty() {
                continue;
            }

            // Diff output keys are NFC-normalised, but working-log paths may be
            // NFD.  Compute the NFC form once for all lookups in this iteration.
            let nfc_file_path: String = file_path.nfc().collect();

            let rebased_line_attrs;
            let line_attrs = if let Some(snapshot) = &carryover_snapshot {
                let carryover_content = snapshot
                    .get(&nfc_file_path)
                    .or_else(|| snapshot.get(file_path))
                    .ok_or_else(|| {
                        GitAiError::Generic(format!(
                            "carryover snapshot missing content for {}",
                            file_path
                        ))
                    })?;
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

            // Get unstaged lines for this file (in working directory coordinates).
            let mut unstaged_lines: Vec<u32> = Vec::new();
            let unstaged_lookup = unstaged_hunks.get(&nfc_file_path).or_else(|| {
                rename_map
                    .get(&nfc_file_path)
                    .and_then(|np| unstaged_hunks.get(np))
            });
            if let Some(unstaged_ranges) = unstaged_lookup {
                for range in unstaged_ranges {
                    unstaged_lines.extend(range.expand());
                }
                unstaged_lines.sort_unstable();
            }

            // Split line attributions into committed and uncommitted
            // VirtualAttributions has line numbers in working directory coordinates,
            // so we need to convert to commit coordinates before comparing with committed hunks
            let mut committed_lines_map: StdHashMap<String, Vec<u32>> = StdHashMap::new();
            let mut uncommitted_lines_map: StdHashMap<String, Vec<u32>> = StdHashMap::new();

            // Get the committed hunks for this file (if any) - these are in commit coordinates.
            // If the file was renamed, committed_hunks is keyed by the new path.
            let file_committed_hunks = committed_hunks.get(&nfc_file_path).or_else(|| {
                rename_map
                    .get(&nfc_file_path)
                    .and_then(|np| committed_hunks.get(np))
            });

            for line_attr in line_attrs {
                // Check each line individually
                for workdir_line_num in line_attr.start_line..=line_attr.end_line {
                    // Check if this line is unstaged (in working directory but not in commit)
                    let is_unstaged = unstaged_lines.binary_search(&workdir_line_num).is_ok();

                    if is_unstaged {
                        // Line is unstaged, mark as uncommitted
                        uncommitted_lines_map
                            .entry(line_attr.author_id.clone())
                            .or_default()
                            .push(workdir_line_num);
                        referenced_prompts.insert(line_attr.author_id.clone());
                    } else {
                        // Convert working directory line number to commit line number
                        // by subtracting the count of unstaged lines before this line
                        let adjustment = unstaged_lines
                            .iter()
                            .filter(|&&l| l < workdir_line_num)
                            .count() as u32;
                        let commit_line_num = workdir_line_num - adjustment;

                        // Check if this commit line number is in any committed hunk
                        let is_committed = if let Some(hunks) = file_committed_hunks {
                            hunks.iter().any(|hunk| hunk.contains(commit_line_num))
                        } else {
                            false
                        };

                        let is_renamed_file = rename_map.contains_key(&nfc_file_path);

                        if is_committed {
                            // Line was committed in this commit (use commit coordinates)
                            committed_lines_map
                                .entry(line_attr.author_id.clone())
                                .or_default()
                                .push(commit_line_num);
                        } else if is_renamed_file
                            && line_attr.author_id != CheckpointKind::Human.to_str()
                            && !line_attr.author_id.starts_with("h_")
                        {
                            // For renamed files, git blame attributes ALL lines to
                            // this commit. Include AI lines in the note even if they're
                            // not in committed_hunks — without this, they'd have no
                            // attestation and blame would fall back to the git committer.
                            committed_lines_map
                                .entry(line_attr.author_id.clone())
                                .or_default()
                                .push(commit_line_num);
                        }
                    }
                }
            }

            // Fill gaps in committed hunks caused by imara_diff Equal matching.
            //
            // When AI rewrites a region, imara_diff can match byte-for-byte
            // identical lines (e.g. empty lines between code blocks) as "Equal",
            // preserving the old human attribution. Those lines get stripped from
            // the checkpoint's line_attributions and never make it here. This
            // leaves gaps in committed_hunks that show as [no-data] in `git ai diff`.
            //
            // Fix: for each gap line in a committed hunk, check the nearest
            // attributed line before and after it. If both neighbors have the
            // same AI author (not human/h_), fill the gap with that author.
            if let Some(hunks) = file_committed_hunks {
                // Build a sorted map of committed line → author_id for neighbor lookups
                let mut line_to_author: Vec<(u32, &str)> = Vec::new();
                for (author_id, lines) in &committed_lines_map {
                    for &line in lines {
                        line_to_author.push((line, author_id.as_str()));
                    }
                }
                line_to_author.sort_by_key(|(line, _)| *line);

                let mut gap_fills: Vec<(String, u32)> = Vec::new();

                // Read file content for content-based gap matching
                let gap_file_content = self
                    .file_contents
                    .get(file_path)
                    .or_else(|| self.file_contents.get(&nfc_file_path));
                let gap_file_lines: Vec<&str> = gap_file_content
                    .map(|c| c.lines().collect())
                    .unwrap_or_default();

                // Build content→author map from AI-attributed lines
                let mut content_to_ai_author: StdHashMap<&str, &str> = StdHashMap::new();
                if !gap_file_lines.is_empty() {
                    for &(line_num, author) in &line_to_author {
                        if !author.starts_with("h_")
                            && author != CheckpointKind::Human.to_str()
                            && let Some(&content) = gap_file_lines.get((line_num - 1) as usize)
                            && !content.trim().is_empty()
                        {
                            content_to_ai_author.insert(content, author);
                        }
                    }
                }

                for hunk in hunks {
                    for line in hunk.expand() {
                        // Skip lines that already have attribution
                        if line_to_author
                            .binary_search_by_key(&line, |(l, _)| *l)
                            .is_ok()
                        {
                            continue;
                        }

                        // Find nearest attributed neighbor before this line
                        let prev = line_to_author.iter().rev().find(|(l, _)| *l < line);

                        // Find nearest attributed neighbor after this line
                        let next = line_to_author.iter().find(|(l, _)| *l > line);

                        // Fill if both neighbors exist and are the same AI author
                        if let (Some((_, prev_author)), Some((_, next_author))) = (prev, next)
                            && prev_author == next_author
                            && !prev_author.starts_with("h_")
                        {
                            gap_fills.push((prev_author.to_string(), line));
                        } else if let Some(&content) = gap_file_lines.get((line - 1) as usize) {
                            // Content-based fallback: if the gap line has the same
                            // content as an AI-attributed line in this file, it's
                            // likely part of the same AI edit (imara_diff matched it
                            // as Equal against old content by mistake).
                            if let Some(&author) = content_to_ai_author.get(content) {
                                gap_fills.push((author.to_string(), line));
                            }
                        }
                    }
                }

                for (author_id, line) in gap_fills {
                    committed_lines_map.entry(author_id).or_default().push(line);
                }
            }

            // Add committed attributions to authorship log
            if !committed_lines_map.is_empty() {
                // Create attestation entries from committed lines
                for (author_id, mut lines) in committed_lines_map {
                    // Skip the legacy "human" sentinel (CheckpointKind::Human checkpoints that were
                    // never attested). KnownHuman lines use h_-prefixed author IDs and pass through.
                    if author_id == CheckpointKind::Human.to_str() {
                        continue;
                    }

                    lines.sort();
                    lines.dedup();

                    if lines.is_empty() {
                        continue;
                    }

                    // Create line ranges
                    let mut ranges = Vec::new();
                    let mut range_start = lines[0];
                    let mut range_end = lines[0];

                    for &line in &lines[1..] {
                        if line == range_end + 1 {
                            range_end = line;
                        } else {
                            if range_start == range_end {
                                ranges.push(crate::model::authorship_log::LineRange::Single(
                                    range_start,
                                ));
                            } else {
                                ranges.push(crate::model::authorship_log::LineRange::Range(
                                    range_start,
                                    range_end,
                                ));
                            }
                            range_start = line;
                            range_end = line;
                        }
                    }

                    // Add the last range
                    if range_start == range_end {
                        ranges.push(crate::model::authorship_log::LineRange::Single(range_start));
                    } else {
                        ranges.push(crate::model::authorship_log::LineRange::Range(
                            range_start,
                            range_end,
                        ));
                    }

                    let entry = crate::model::authorship_log_serialization::AttestationEntry::new(
                        author_id, ranges,
                    );

                    let attestation_path = rename_map.get(&nfc_file_path).unwrap_or(&nfc_file_path);
                    let file_attestation = authorship_log.get_or_create_file(attestation_path);
                    file_attestation.add_entry(entry);
                }
            }

            // Add uncommitted attributions to INITIAL
            if !uncommitted_lines_map.is_empty() {
                // Convert the map into line attributions
                let mut uncommitted_line_attrs = Vec::new();
                for (author_id, mut lines) in uncommitted_lines_map {
                    // Skip the legacy "human" sentinel (CheckpointKind::Human checkpoints that were
                    // never attested). KnownHuman lines use h_-prefixed author IDs and pass through.
                    if author_id == CheckpointKind::Human.to_str() {
                        continue;
                    }

                    lines.sort();
                    lines.dedup();

                    if lines.is_empty() {
                        continue;
                    }

                    // Track h_ hashes for INITIAL humans map
                    if author_id.starts_with("h_") {
                        // h_ hash absent from self.humans — foreign cherry-pick or pre-existing
                        // INITIAL attribution. Intentionally skip: the record is not needed locally.
                        if let Some(record) = self.humans.get(&author_id) {
                            initial_humans.insert(author_id.clone(), record.clone());
                        }
                    }

                    // Track s_ sessions for INITIAL sessions map
                    if author_id.starts_with("s_") {
                        let session_key = author_id
                            .split("::")
                            .next()
                            .unwrap_or(&author_id)
                            .to_string();
                        if let Some(record) = self.sessions.get(&session_key) {
                            initial_sessions.insert(session_key, record.clone());
                        }
                    }

                    // Create ranges from individual lines
                    let mut range_start = lines[0];
                    let mut range_end = lines[0];

                    for &line in &lines[1..] {
                        if line == range_end + 1 {
                            range_end = line;
                        } else {
                            // End current range and start new one
                            uncommitted_line_attrs.push(LineAttribution {
                                start_line: range_start,
                                end_line: range_end,
                                author_id: author_id.clone(),
                                overrode: None,
                            });
                            range_start = line;
                            range_end = line;
                        }
                    }

                    // Add the last range
                    uncommitted_line_attrs.push(LineAttribution {
                        start_line: range_start,
                        end_line: range_end,
                        author_id: author_id.clone(),
                        overrode: None,
                    });
                }

                let initial_path = rename_map.get(file_path).unwrap_or(file_path);
                initial_files.insert(initial_path.clone(), uncommitted_line_attrs);
                if let Some(snapshot) = &carryover_snapshot {
                    if let Some(content) = snapshot
                        .get(initial_path)
                        .or_else(|| snapshot.get(file_path))
                    {
                        initial_file_contents.insert(initial_path.clone(), content.clone());
                    }
                } else if let Some(content) = self
                    .file_contents
                    .get(file_path)
                    .or_else(|| self.file_contents.get(&nfc_file_path))
                {
                    initial_file_contents.insert(initial_path.clone(), content.clone());
                }
            }
        }

        // Remove INITIAL-only prompts that have no committed lines in the
        // attestations.  Prompts originating from current-session checkpoints are
        // kept unconditionally (they represent AI tools used during development,
        // even if their lines didn't land — the "non-landing prompt" feature).
        // Only INITIAL-carried prompts (from prior commits' uncommitted AI lines)
        // are filtered out when they have no committed lines.
        if !self.initial_only_prompt_ids.is_empty() {
            let committed_prompt_ids: HashSet<&String> = authorship_log
                .attestations
                .iter()
                .flat_map(|file_att| file_att.entries.iter())
                .map(|entry| &entry.hash)
                .collect();
            authorship_log.metadata.prompts.retain(|prompt_id, _| {
                // Keep if: not INITIAL-only, OR has committed lines
                !self.initial_only_prompt_ids.contains(prompt_id)
                    || committed_prompt_ids.contains(prompt_id)
            });
        }

        // Prune sessions that have no corresponding attestation entries.
        // Unlike prompts (which keep "non-landing" records for historical reasons),
        // sessions are only retained if at least one attestation references them.
        {
            let committed_session_ids: HashSet<String> = authorship_log
                .attestations
                .iter()
                .flat_map(|file_att| file_att.entries.iter())
                .filter_map(|entry| {
                    if entry.hash.starts_with("s_") {
                        Some(
                            entry
                                .hash
                                .split("::")
                                .next()
                                .unwrap_or(&entry.hash)
                                .to_string(),
                        )
                    } else {
                        None
                    }
                })
                .collect();

            authorship_log
                .metadata
                .sessions
                .retain(|session_id, _| committed_session_ids.contains(session_id));
        }

        // Build prompts map for INITIAL (only prompts referenced by uncommitted lines)
        let mut initial_prompts = StdHashMap::new();
        for prompt_id in referenced_prompts {
            if let Some(commits) = self.prompts.get(&prompt_id) {
                // Get the most recent (first) prompt for this prompt_id
                if let Some(prompt) = commits.values().next() {
                    initial_prompts.insert(prompt_id, prompt.clone());
                }
            }
        }

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
