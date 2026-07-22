//! Attribution computation from a pre-loaded [`AuthorshipLog`] (fast path).
//!
//! When a caller already has an authorship note for the target commit, this
//! module can build per-line attributions without running `git blame`.

use crate::model::authorship_log::{HumanRecord, LineRange, PromptRecord, SessionRecord};
use crate::model::authorship_log_serialization::AuthorshipLog;
use crate::operations::commands::diff::{Attribution, DiffHunk, DiffLineKey, LineSide};
use crate::operations::commands::diff_attribution::{LineAttributionDetail, collect_lines_by_file};
use crate::operations::commands::diff_json_builder::extract_session_id;
use std::collections::{BTreeMap, HashMap};

/// Build per-line attributions from a pre-loaded authorship log, returning the
/// same tuple as `build_line_attribution_data`.
#[allow(clippy::type_complexity)]
pub(crate) fn build_line_attribution_from_note(
    to_commit: &str,
    hunks: &[DiffHunk],
    note: &AuthorshipLog,
) -> (
    BTreeMap<String, BTreeMap<String, Vec<LineRange>>>,
    HashMap<DiffLineKey, Attribution>,
    HashMap<DiffLineKey, LineAttributionDetail>,
    BTreeMap<String, PromptRecord>,
    BTreeMap<String, SessionRecord>,
    BTreeMap<String, HumanRecord>,
    BTreeMap<String, crate::operations::commands::diff::DiffCommitMetadata>,
) {
    let mut annotations_by_file: BTreeMap<String, BTreeMap<String, Vec<LineRange>>> =
        BTreeMap::new();
    let mut attributions: HashMap<DiffLineKey, Attribution> = HashMap::new();
    let mut line_details: HashMap<DiffLineKey, LineAttributionDetail> = HashMap::new();
    let prompts: BTreeMap<String, PromptRecord> = note.metadata.prompts.clone();
    let sessions: BTreeMap<String, SessionRecord> = note.metadata.sessions.clone();
    let humans: BTreeMap<String, HumanRecord> = note.metadata.humans.clone();
    let commits: BTreeMap<String, crate::operations::commands::diff::DiffCommitMetadata> =
        BTreeMap::new();

    let added_lines_by_file = collect_lines_by_file(hunks, LineSide::New);
    for (file_path, lines) in &added_lines_by_file {
        let file_attestation = note
            .attestations
            .iter()
            .find(|fa| &fa.file_path == file_path);

        let mut file_annotations: BTreeMap<String, Vec<LineRange>> = BTreeMap::new();

        for line in lines {
            let key = DiffLineKey {
                file: file_path.clone(),
                line: *line,
                side: LineSide::New,
            };

            let mut found_hash: Option<&str> = None;
            if let Some(fa) = file_attestation {
                for entry in &fa.entries {
                    if entry.line_ranges.iter().any(|r| r.contains(*line)) {
                        found_hash = Some(&entry.hash);
                        break;
                    }
                }
            }

            if let Some(hash) = found_hash {
                let is_prompt = prompts.contains_key(hash) || hash.starts_with("s_");
                let is_human = hash.starts_with("h_");

                let (prompt_id, human_id, attribution) = if is_prompt {
                    let tool = prompts
                        .get(hash)
                        .map(|p| p.agent_id.tool.clone())
                        .unwrap_or_else(|| {
                            let session_key = extract_session_id(hash);
                            sessions
                                .get(session_key)
                                .map(|s| s.agent_id.tool.clone())
                                .unwrap_or_else(|| "unknown".to_string())
                        });
                    (Some(hash.to_string()), None, Attribution::Ai(tool))
                } else if is_human {
                    (
                        None,
                        Some(hash.to_string()),
                        Attribution::Human(hash.to_string()),
                    )
                } else {
                    (None, None, Attribution::NoData)
                };

                if let Some(ref pid) = prompt_id {
                    file_annotations
                        .entry(pid.clone())
                        .or_default()
                        .push(LineRange::Single(*line));
                }

                attributions.insert(key.clone(), attribution);
                line_details.insert(
                    key,
                    LineAttributionDetail {
                        commit_sha: Some(to_commit.to_string()),
                        prompt_id,
                        human_id,
                    },
                );
            } else {
                attributions.insert(key.clone(), Attribution::NoData);
                line_details.insert(
                    key,
                    LineAttributionDetail {
                        commit_sha: Some(to_commit.to_string()),
                        prompt_id: None,
                        human_id: None,
                    },
                );
            }
        }

        if !file_annotations.is_empty() {
            annotations_by_file.insert(file_path.clone(), file_annotations);
        }
    }

    // Deleted lines: include with NoData attribution (no blame).
    // Use file_path (new name) for DiffLineKey to match build_json_hunk_segments lookup.
    for hunk in hunks {
        for line in &hunk.deleted_lines {
            let key = DiffLineKey {
                file: hunk.file_path.clone(),
                line: *line,
                side: LineSide::Old,
            };
            attributions.insert(key.clone(), Attribution::NoData);
            line_details.insert(
                key,
                LineAttributionDetail {
                    commit_sha: None,
                    prompt_id: None,
                    human_id: None,
                },
            );
        }
    }

    (
        annotations_by_file,
        attributions,
        line_details,
        prompts,
        sessions,
        humans,
        commits,
    )
}
