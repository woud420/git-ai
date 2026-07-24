//! Shared event-dispatch tail for the "claude-wire" family of checkpoint
//! presets: claude, gemini, continue_cli, droid, opencode, firebender,
//! cline, cursor (both presets), and windsurf.
//!
//! These presets all resolve a hook payload down to the same four pieces —
//! "is this a pre- or post-tool event", "is the tool bash or a file edit",
//! plus the already-built `PresetContext`/`StreamSource` — and then pick one
//! of the four `ParsedHookEvent` variants. That final selection was
//! previously duplicated as a near-identical `match (is_pre, is_bash) { ... }`
//! (or an equivalent if/else chain) in every preset's `parse()`. This module
//! extracts just that mechanical tail.
//!
//! Everything upstream of the dispatch — guards, session_id/model/transcript
//! resolution, tool classification — stays local to each preset because it
//! genuinely differs preset to preset (different field names, different
//! fallback chains, different model-extraction strategies). Folding that
//! part into a shared helper would require enough per-preset parameterization
//! to make the abstraction harder to read than the duplication it removes.
//!
//! `ai_tab` (no bash concept; wants `tool_use_id: None` rather than
//! `Some(id)`) and `pi` (validation and field-sourcing embedded per variant,
//! and a raw `Option<String>` tool_use_id passthrough on file events) do not
//! fit this shape and are intentionally left concrete.

use super::{
    ParsedHookEvent, PostBashCall, PostFileEdit, PreBashCall, PreFileEdit, PresetContext,
    StreamSource,
};
use std::collections::HashMap;
use std::path::PathBuf;

/// Build the `ParsedHookEvent` for a claude-wire preset from its already
/// resolved fields.
///
/// `tool_use_id` is used as-is for the bash variants and as `Some(tool_use_id)`
/// for the file-edit variants — every consolidated preset already threads the
/// same id through both cases.
#[allow(clippy::too_many_arguments)]
pub(super) fn build_wire_event(
    is_pre: bool,
    is_bash: bool,
    context: PresetContext,
    tool_use_id: String,
    command: Option<String>,
    file_paths: Vec<PathBuf>,
    dirty_files: Option<HashMap<PathBuf, String>>,
    stream_source: Option<StreamSource>,
) -> ParsedHookEvent {
    match (is_pre, is_bash) {
        (true, true) => ParsedHookEvent::PreBashCall(PreBashCall {
            context,
            tool_use_id,
            command,
        }),
        (true, false) => ParsedHookEvent::PreFileEdit(PreFileEdit {
            context,
            file_paths,
            dirty_files,
            tool_use_id: Some(tool_use_id),
        }),
        (false, true) => ParsedHookEvent::PostBashCall(PostBashCall {
            context,
            tool_use_id,
            command,
            stream_source,
        }),
        (false, false) => ParsedHookEvent::PostFileEdit(PostFileEdit {
            context,
            file_paths,
            dirty_files,
            stream_source,
            tool_use_id: Some(tool_use_id),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::working_log::AgentId;

    fn context() -> PresetContext {
        PresetContext {
            agent_id: AgentId {
                tool: "test-tool".to_string(),
                id: "sess-1".to_string(),
                model: "unknown".to_string(),
            },
            external_session_id: "sess-1".to_string(),
            trace_id: "t_test".to_string(),
            cwd: PathBuf::from("/home/user/project"),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn pre_bash_uses_tool_use_id_and_command_as_is() {
        let event = build_wire_event(
            true,
            true,
            context(),
            "tu-1".to_string(),
            Some("echo hi".to_string()),
            vec![],
            None,
            None,
        );
        match event {
            ParsedHookEvent::PreBashCall(e) => {
                assert_eq!(e.tool_use_id, "tu-1");
                assert_eq!(e.command.as_deref(), Some("echo hi"));
            }
            other => panic!("expected PreBashCall, got {other:?}"),
        }
    }

    #[test]
    fn post_bash_carries_stream_source() {
        let stream_source = StreamSource {
            path: PathBuf::from("/tmp/transcript.jsonl"),
            format: super::super::StreamFormat::ClaudeJsonl,
            session_id: "gen-1".to_string(),
            external_session_id: "sess-1".to_string(),
            external_parent_session_id: None,
        };
        let event = build_wire_event(
            false,
            true,
            context(),
            "tu-2".to_string(),
            None,
            vec![],
            None,
            Some(stream_source.clone()),
        );
        match event {
            ParsedHookEvent::PostBashCall(e) => {
                assert_eq!(e.tool_use_id, "tu-2");
                assert!(e.command.is_none());
                assert_eq!(
                    e.stream_source.map(|s| s.session_id),
                    Some(stream_source.session_id)
                );
            }
            other => panic!("expected PostBashCall, got {other:?}"),
        }
    }

    #[test]
    fn pre_file_edit_wraps_tool_use_id_in_some() {
        let paths = vec![PathBuf::from("/home/user/project/src/main.rs")];
        let event = build_wire_event(
            true,
            false,
            context(),
            "tu-3".to_string(),
            None,
            paths.clone(),
            None,
            None,
        );
        match event {
            ParsedHookEvent::PreFileEdit(e) => {
                assert_eq!(e.file_paths, paths);
                assert_eq!(e.tool_use_id, Some("tu-3".to_string()));
                assert!(e.dirty_files.is_none());
            }
            other => panic!("expected PreFileEdit, got {other:?}"),
        }
    }

    #[test]
    fn pre_file_edit_preserves_some_dirty_files() {
        // firebender is the one preset that sends Some(dirty) on pre-file events;
        // a helper hardcoding None here would pass every other test.
        let paths = vec![PathBuf::from("/home/user/project/src/main.rs")];
        let mut dirty = HashMap::new();
        dirty.insert(
            PathBuf::from("/home/user/project/src/lib.rs"),
            "dirty-content".to_string(),
        );
        let event = build_wire_event(
            true,
            false,
            context(),
            "tu-5".to_string(),
            None,
            paths.clone(),
            Some(dirty.clone()),
            None,
        );
        match event {
            ParsedHookEvent::PreFileEdit(e) => {
                assert_eq!(e.file_paths, paths);
                assert_eq!(e.dirty_files, Some(dirty));
                assert_eq!(e.tool_use_id, Some("tu-5".to_string()));
            }
            other => panic!("expected PreFileEdit, got {other:?}"),
        }
    }

    #[test]
    fn post_file_edit_carries_dirty_files_and_stream_source() {
        let mut dirty = HashMap::new();
        dirty.insert(PathBuf::from("src/main.rs"), "old content".to_string());
        let event = build_wire_event(
            false,
            false,
            context(),
            "tu-4".to_string(),
            None,
            vec![PathBuf::from("src/main.rs")],
            Some(dirty.clone()),
            None,
        );
        match event {
            ParsedHookEvent::PostFileEdit(e) => {
                assert_eq!(e.tool_use_id, Some("tu-4".to_string()));
                assert_eq!(e.dirty_files, Some(dirty));
                assert!(e.stream_source.is_none());
            }
            other => panic!("expected PostFileEdit, got {other:?}"),
        }
    }
}
