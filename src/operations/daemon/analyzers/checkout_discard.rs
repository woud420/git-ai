use super::restore_discard::head_path_discard_event;
use crate::model::domain::{NormalizedCommand, SemanticEvent};
use std::collections::HashMap;

pub(crate) fn event(
    cmd: &NormalizedCommand,
    refs: &HashMap<String, String>,
) -> Option<SemanticEvent> {
    head_path_discard_event(cmd, refs, "checkout", |args| match args {
        [source, separator, path] if separator == "--" => Some((source, path)),
        _ => None,
    })
}
