use super::*;

pub(super) fn pull_reflog_action_state<'a>(message: &'a str, action: &str) -> Option<&'a str> {
    let rest = message.strip_prefix(action)?;
    let open = rest.find('(')?;
    let after_open = &rest[open + 1..];
    let close = after_open.find("):")?;
    Some(&after_open[..close])
}

pub(super) fn pull_reflog_action_is(message: &str, action: &str, expected: &str) -> bool {
    pull_reflog_action_state(message, action).is_some_and(|state| state == expected)
}

pub(super) fn pull_reflog_action_starts_new_command(message: &str, action: &str) -> bool {
    matches!(
        pull_reflog_action_state(message, action),
        Some("start" | "continue" | "skip" | "abort" | "quit" | "finish")
    )
}

pub(super) fn rebase_reflog_action(message: &str) -> Option<&str> {
    let rest = message.strip_prefix("rebase")?;
    let open = rest.find('(')?;
    let after_open = &rest[open + 1..];
    let close = after_open.find("):")?;
    Some(&after_open[..close])
}

pub(super) fn rebase_reflog_action_is(message: &str, expected: &str) -> bool {
    rebase_reflog_action(message).is_some_and(|action| action == expected)
}

pub(super) fn rebase_reflog_action_starts_new_command(message: &str) -> bool {
    matches!(
        rebase_reflog_action(message),
        Some("start" | "continue" | "skip" | "abort" | "quit" | "finish")
    )
}

pub(super) fn clamp_seed_to_entry_containing_offset(
    entries: &[CursorEntry],
    offset: u64,
    message_prefixes: &[&str],
) -> Option<u64> {
    // A matching row after the offset means the offset is a real command-start
    // boundary before the current command's reflog write. Do not rewind into
    // older history in that case.
    if entries.iter().any(|entry| {
        entry.start_offset >= offset && message_matches(&entry.message, message_prefixes)
    }) {
        return None;
    }

    // If the asynchronous offset landed at EOF or in the middle of the command's
    // own branch-finish row, seed from the row start so the parser never begins
    // inside a reflog record.
    entries
        .iter()
        .rev()
        .find(|entry| {
            entry.start_offset < offset
                && offset <= entry.end_offset
                && message_matches(&entry.message, message_prefixes)
        })
        .map(|entry| entry.start_offset)
}

pub(super) fn head_span_start_near_offset(
    entries: &[CursorEntry],
    offset: u64,
    message_prefixes: &[&str],
    expected: ExpectedTransition,
    limit: usize,
) -> Option<u64> {
    let mut contiguous = VecDeque::<&CursorEntry>::new();
    let mut latest_before_offset: Option<u64> = None;

    for entry in entries {
        if !message_matches(&entry.message, message_prefixes) {
            contiguous.clear();
            continue;
        }
        if contiguous
            .back()
            .is_some_and(|previous| previous.new != entry.old)
        {
            contiguous.clear();
        }
        contiguous.push_back(entry);
        while contiguous.len() > limit {
            contiguous.pop_front();
        }
        if !expected.matches(entry) {
            continue;
        }
        let Some(candidate) = contiguous.front() else {
            continue;
        };
        if candidate.start_offset >= offset {
            return None;
        }
        latest_before_offset = Some(candidate.start_offset);
    }

    latest_before_offset
}

pub(super) fn pull_span_start_containing_offset(
    entries: &[CursorEntry],
    offset: u64,
    action: &str,
    expected: ExpectedTransition,
) -> Option<u64> {
    // If a rebase/pull start row exists after the offset, the offset is a true
    // boundary before the current span. Keep it as-is.
    if entries.iter().any(|entry| {
        entry.start_offset >= offset && pull_reflog_action_is(&entry.message, action, "start")
    }) {
        return None;
    }

    entries
        .iter()
        .enumerate()
        .rev()
        .find(|(idx, entry)| {
            entry.start_offset < offset
                && pull_reflog_action_is(&entry.message, action, "start")
                && expected.matches_span_boundary(entry)
                && pull_span_covers_offset(entries, *idx, offset, action)
        })
        .map(|(_, entry)| entry.start_offset)
}

pub(super) fn pull_span_covers_offset(
    entries: &[CursorEntry],
    start_idx: usize,
    offset: u64,
    action: &str,
) -> bool {
    for entry in entries.iter().skip(start_idx + 1) {
        if entry.start_offset >= offset {
            return true;
        }
        if pull_reflog_action_starts_new_command(&entry.message, action) {
            return offset <= entry.end_offset;
        }
    }
    true
}

pub(super) fn rebase_span_start_containing_offset(
    entries: &[CursorEntry],
    offset: u64,
    expected: ExpectedTransition,
) -> Option<u64> {
    // If a rebase start row exists after the offset, the offset is a true
    // boundary before the current span. Keep it as-is.
    if entries.iter().any(|entry| {
        entry.start_offset >= offset && rebase_reflog_action_is(&entry.message, "start")
    }) {
        return None;
    }

    entries
        .iter()
        .enumerate()
        .rev()
        .find(|(idx, entry)| {
            entry.start_offset < offset
                && rebase_reflog_action_is(&entry.message, "start")
                && expected.matches_span_boundary(entry)
                && rebase_span_covers_offset(entries, *idx, offset)
        })
        .map(|(_, entry)| entry.start_offset)
}

pub(super) fn rebase_span_covers_offset(
    entries: &[CursorEntry],
    start_idx: usize,
    offset: u64,
) -> bool {
    for entry in entries.iter().skip(start_idx + 1) {
        if entry.start_offset >= offset {
            return true;
        }
        if rebase_reflog_action_starts_new_command(&entry.message) {
            return offset <= entry.end_offset;
        }
    }
    true
}
