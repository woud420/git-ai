use crate::error::GitAiError;
use crate::model::attribution_tracker::{LineAttribution, attributions_to_line_attributions};
use crate::model::authorship_log::LineRange;
use crate::model::authorship_log_serialization::{AttestationEntry, AuthorshipLog};
use crate::model::imara_diff_utils::{
    DiffOp, capture_diff_slices, content_eq_ignoring_line_endings, normalize_line_endings,
};
use crate::model::working_log::{Checkpoint, InitialAttributions, WorkingLogEntry};
use crate::operations::authorship::virtual_attribution::{
    VirtualAttributions, diff_hunks_between_contents,
};
use crate::operations::git::repo_storage::PersistedWorkingLog;
use crate::operations::git::repository::{Repository, batch_read_paths_at_treeishes};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

pub(super) struct CheckpointEvidence<'a> {
    pub working_log: &'a PersistedWorkingLog,
    pub checkpoints: &'a [Checkpoint],
    pub initial: &'a InitialAttributions,
    pub attribution_metadata: &'a VirtualAttributions,
    pub observed: &'a HashMap<String, String>,
}

impl CheckpointEvidence<'_> {
    pub(super) fn reconcile(
        &self,
        repo: &Repository,
        parent: &str,
        commit: &str,
        log: &mut AuthorshipLog,
        recovery_hunks: &mut HashMap<String, Vec<LineRange>>,
    ) -> Result<(), GitAiError> {
        let observed: HashMap<String, _> = self
            .observed
            .iter()
            .map(|(path, content)| (path.nfc().collect(), (path, content)))
            .collect();
        let paths: Vec<_> = log
            .attestations
            .iter()
            .filter(|file| recovery_hunks.contains_key(&file.file_path))
            .filter_map(|file| observed.get(&file.file_path).map(|(path, _)| *path))
            .collect();
        if paths.is_empty() {
            return Ok(());
        }

        // Both immutable trees are read in one batch, never once per path.
        let requests: Vec<_> = paths
            .iter()
            .flat_map(|path| {
                [
                    (parent.to_owned(), (*path).clone()),
                    (commit.to_owned(), (*path).clone()),
                ]
            })
            .collect();
        let contents = batch_read_paths_at_treeishes(repo, &requests)?;
        let mut history: HashMap<String, Vec<&WorkingLogEntry>> = HashMap::new();
        for checkpoint in self.checkpoints {
            for entry in &checkpoint.entries {
                history
                    .entry(entry.file.nfc().collect())
                    .or_default()
                    .push(entry);
            }
        }

        let mut restored_authors = HashSet::new();
        for file in &mut log.attestations {
            let Some(ranges) = recovery_hunks.get_mut(&file.file_path) else {
                continue;
            };
            let Some((path, observed_content)) = observed.get(&file.file_path) else {
                continue;
            };
            let Some(committed) = contents.get(&(commit.to_owned(), (*path).clone())) else {
                continue;
            };
            if content_eq_ignoring_line_endings(observed_content, committed) {
                continue;
            }
            let parent_content = contents
                .get(&(parent.to_owned(), (*path).clone()))
                .map(String::as_str)
                .unwrap_or_default();
            let committed_lines: HashSet<_> = ranges.iter().flat_map(LineRange::expand).collect();
            let modified: HashSet<_> = diff_hunks_between_contents(parent_content, committed)
                .into_iter()
                .filter(|hunk| hunk.old_count > 0)
                .flat_map(|hunk| hunk.new_start..hunk.new_start.saturating_add(hunk.new_count))
                .collect();
            let changed: Vec<_> = diff_hunks_between_contents(observed_content, committed)
                .into_iter()
                .filter(|hunk| hunk.old_count > 0 && hunk.old_count == hunk.new_count)
                .flat_map(|hunk| hunk.new_start..hunk.new_start.saturating_add(hunk.new_count))
                .filter(|line| committed_lines.contains(line) && modified.contains(line))
                .collect();
            if changed.is_empty() {
                continue;
            }

            let checkpoint_attrs = self.historical_attributions(
                path,
                committed,
                &changed,
                history
                    .get(&file.file_path)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            )?;
            let changed_ranges = LineRange::compress_lines(&changed);
            for entry in &mut file.entries {
                entry.remove_line_ranges(&changed_ranges);
            }
            file.entries.retain(|entry| !entry.line_ranges.is_empty());
            {
                let mut authors: BTreeMap<String, Vec<u32>> = BTreeMap::new();
                for attr in checkpoint_attrs {
                    if attr.author_id == "human" {
                        continue;
                    }
                    authors
                        .entry(attr.author_id)
                        .or_default()
                        .extend(attr.start_line..=attr.end_line);
                }
                for (author, mut lines) in authors {
                    lines.sort_unstable();
                    lines.dedup();
                    if !lines.is_empty() {
                        restored_authors.insert(author.clone());
                        file.add_entry(AttestationEntry::new(
                            author,
                            LineRange::compress_lines(&lines),
                        ));
                    }
                }
            }
            // Invalidated evidence must not be inferred back from adjacent AI lines
            // or timestamp recovery. Absence of evidence remains untracked.
            for removed in &changed_ranges {
                *ranges = ranges
                    .iter()
                    .flat_map(|range| range.remove(removed))
                    .collect();
            }
        }
        // Projection prunes metadata for unstaged sessions. Restoring an earlier
        // checkpoint must restore its identity as well as its line ranges.
        for author in restored_authors {
            let session_id = author.split("::").next().unwrap_or(&author);
            if let Some(session) = self.attribution_metadata.sessions.get(session_id) {
                log.metadata
                    .sessions
                    .entry(session_id.to_owned())
                    .or_insert_with(|| session.clone());
            }
            if let Some(human) = self.attribution_metadata.humans.get(&author) {
                log.metadata
                    .humans
                    .entry(author.clone())
                    .or_insert_with(|| human.clone());
            }
            if let Some(prompt) = self
                .attribution_metadata
                .prompts
                .get(&author)
                .and_then(|commits| commits.values().next())
            {
                log.metadata
                    .prompts
                    .entry(author)
                    .or_insert_with(|| prompt.clone());
            }
        }
        log.attestations.retain(|file| !file.entries.is_empty());
        recovery_hunks.retain(|_, ranges| !ranges.is_empty());
        Ok(())
    }

    fn historical_attributions(
        &self,
        path: &str,
        committed: &str,
        changed: &[u32],
        entries: &[&WorkingLogEntry],
    ) -> Result<Vec<LineAttribution>, GitAiError> {
        let mut unresolved: BTreeSet<_> = changed.iter().copied().collect();
        let mut restored = Vec::new();
        let mut seen = HashSet::new();
        for entry in entries.iter().rev() {
            if unresolved.is_empty() {
                break;
            }
            if entry.blob_sha.is_empty() || !seen.insert(&entry.blob_sha) {
                continue;
            }
            let content = self.working_log.get_file_version(&entry.blob_sha)?;
            let attrs = if entry.line_attributions.is_empty() {
                attributions_to_line_attributions(&entry.attributions, &content)
            } else {
                entry.line_attributions.clone()
            };
            restore_matching_lines(&content, committed, &attrs, &mut unresolved, &mut restored);
        }
        if !unresolved.is_empty()
            && let Some(blob) = self.initial.file_blobs.get(path)
            && let Some(attrs) = self.initial.files.get(path)
        {
            let content = self.working_log.get_file_version(blob)?;
            restore_matching_lines(&content, committed, attrs, &mut unresolved, &mut restored);
        }
        Ok(restored)
    }
}

fn restore_matching_lines(
    checkpoint: &str,
    committed: &str,
    attrs: &[LineAttribution],
    unresolved: &mut BTreeSet<u32>,
    restored: &mut Vec<LineAttribution>,
) {
    let checkpoint = normalize_line_endings(checkpoint);
    let committed = normalize_line_endings(committed);
    // An EOF newline change does not replace the text carrying attribution.
    let old_lines: Vec<_> = checkpoint.lines().collect();
    let new_lines: Vec<_> = committed.lines().collect();
    for op in capture_diff_slices(&old_lines, &new_lines) {
        let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        else {
            continue;
        };
        for attr in attrs {
            let start = attr.start_line.max(old_index as u32 + 1);
            let end = attr.end_line.min((old_index + len) as u32);
            if start > end {
                continue;
            }
            let target_start = start - old_index as u32 + new_index as u32;
            let target_end = end - old_index as u32 + new_index as u32;
            let lines: Vec<_> = unresolved
                .range(target_start..=target_end)
                .copied()
                .collect();
            for line in lines {
                unresolved.remove(&line);
                restored.push(LineAttribution {
                    start_line: line,
                    end_line: line,
                    author_id: attr.author_id.clone(),
                    overrode: attr.overrode.clone(),
                });
            }
        }
    }
}
