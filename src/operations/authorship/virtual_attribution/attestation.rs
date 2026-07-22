use super::types::VirtualAttributions;
use crate::error::GitAiError;
use crate::model::attribution_tracker::LineAttribution;
use crate::model::authorship_log::LineRange;
use crate::model::working_log::CheckpointKind;
use std::collections::HashMap;
use unicode_normalization::UnicodeNormalization;

impl VirtualAttributions {
    /// Convert this VirtualAttributions to an AuthorshipLog
    pub fn to_authorship_log(
        &self,
    ) -> Result<crate::model::authorship_log_serialization::AuthorshipLog, GitAiError> {
        let mut authorship_log = self.authorship_log_with_metadata();

        authorship_log.attestations = build_attestations_from_attributions(&self.attributions);

        Ok(authorship_log)
    }
}

/// Build the deterministically-ordered attestation list for an authorship log
/// from the per-file (char, line) attribution map.
///
/// `self.attributions` is a `HashMap`, and entries within a file are grouped by
/// a `HashMap<author_id, ranges>`; iterating either directly yields a
/// process-randomised order, which would make byte-identical commits produce
/// different note bytes (breaking idempotent note sync / dedup). We therefore
/// sort files by path and entries by hash so the output is stable. Ranges
/// within an entry are already sorted+merged.
pub(super) fn build_attestations_from_attributions(
    attributions: &HashMap<
        String,
        (
            Vec<crate::model::attribution_tracker::Attribution>,
            Vec<LineAttribution>,
        ),
    >,
) -> Vec<crate::model::authorship_log_serialization::FileAttestation> {
    use crate::model::authorship_log_serialization::{AttestationEntry, FileAttestation};

    let mut files: Vec<FileAttestation> = Vec::new();

    for (file_path, (_, line_attrs)) in attributions {
        if line_attrs.is_empty() {
            continue;
        }

        // Group line attributions by author as intervals.
        // This avoids expanding every range to individual line numbers.
        let mut author_ranges: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
        for line_attr in line_attrs {
            // Skip the legacy "human" sentinel (CheckpointKind::Human checkpoints that were
            // never attested). KnownHuman lines use h_-prefixed author IDs and pass through.
            if line_attr.author_id == CheckpointKind::Human.to_str() {
                continue;
            }

            author_ranges
                .entry(line_attr.author_id.clone())
                .or_default()
                .push((line_attr.start_line, line_attr.end_line));
        }

        // NFC-normalise the path so that attestation file_path is consistent
        // with NFC paths emitted by git diff parsing.
        let nfc_fp: String = file_path.nfc().collect();
        let mut file_attestation = FileAttestation::new(nfc_fp);

        // Create attestation entries for each author.
        for (author_id, mut ranges) in author_ranges {
            if ranges.is_empty() {
                continue;
            }
            ranges.sort_by_key(|(start, end)| (*start, *end));

            let mut merged: Vec<(u32, u32)> = Vec::new();
            for (start, end) in ranges {
                match merged.last_mut() {
                    Some((_, last_end)) if start <= last_end.saturating_add(1) => {
                        *last_end = (*last_end).max(end);
                    }
                    _ => merged.push((start, end)),
                }
            }

            let line_ranges = merged
                .into_iter()
                .map(|(start, end)| {
                    if start == end {
                        LineRange::Single(start)
                    } else {
                        LineRange::Range(start, end)
                    }
                })
                .collect();

            file_attestation.add_entry(AttestationEntry::new(author_id, line_ranges));
        }

        if file_attestation.entries.is_empty() {
            continue;
        }

        // Deterministic entry order within the file: sort by hash (author_id).
        file_attestation.entries.sort_by(|a, b| a.hash.cmp(&b.hash));
        files.push(file_attestation);
    }

    // Deterministic file order: sort by NFC-normalised path.
    files.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    files
}

/// Derive committed (added) line ranges per file from a pre-computed
/// parent→commit `DiffTreeResult`, equivalent to what `collect_committed_hunks`
/// would return for the same pair. The new-side hunk ranges are the lines added
/// by the commit. Filtered by `pathspecs` when provided.
pub(crate) fn committed_hunks_from_diff_result(
    diff: &crate::operations::authorship::rewrite::DiffTreeResult,
    pathspecs: Option<&std::collections::HashSet<String>>,
) -> HashMap<String, Vec<LineRange>> {
    let mut committed_hunks: HashMap<String, Vec<LineRange>> = HashMap::new();
    for (file_path, added_lines) in &diff.added_lines_by_file {
        if let Some(paths) = pathspecs
            && !paths.contains(file_path)
        {
            continue;
        }
        let lines = added_lines
            .iter()
            .copied()
            .filter(|line| *line > 0)
            .collect::<Vec<_>>();
        if !lines.is_empty() {
            committed_hunks.insert(file_path.clone(), LineRange::compress_lines(&lines));
        }
    }
    committed_hunks
}
