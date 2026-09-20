use crate::model::domain::{FamilyState, NormalizedCommand};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

pub(super) fn for_command<'a>(
    state: &'a FamilyState,
    cmd: &NormalizedCommand,
    command_start_refs: &HashMap<String, String>,
    canonical_worktree: Option<&Path>,
) -> Cow<'a, HashMap<String, String>> {
    if super::super::checkout_discard::is_explicit_path_checkout(cmd) {
        // Family HEAD may belong to another linked worktree. Only a prior
        // sequenced transition for this worktree can anchor its discard.
        let head = canonical_worktree
            .or(cmd.worktree.as_deref())
            .and_then(|key| state.worktrees.get(key))
            .and_then(|worktree| worktree.head.as_ref());
        return Cow::Owned(
            head.map(|oid| ("HEAD".to_string(), oid.clone()))
                .into_iter()
                .collect(),
        );
    }
    if command_start_refs.is_empty() {
        Cow::Borrowed(&state.refs)
    } else {
        Cow::Owned(
            state
                .refs
                .iter()
                .chain(command_start_refs)
                .map(|(reference, oid)| (reference.clone(), oid.clone()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{family_state, normalized};
    use super::*;
    use crate::model::domain::WorktreeState;
    use std::path::PathBuf;

    #[test]
    fn checkout_uses_only_its_sequenced_worktree_head() {
        let mut state = family_state();
        state.refs.insert("HEAD".into(), "family-head".into());
        state.worktrees.insert(
            PathBuf::from("/canonical/repo"),
            WorktreeState {
                head: Some("own-head".into()),
                branch: None,
                detached: true,
                last_updated_ns: 1,
            },
        );
        let mut cmd = normalized();
        cmd.primary_command = Some("checkout".into());
        cmd.raw_argv = vec![
            "git".into(),
            "checkout".into(),
            "a".repeat(40),
            "--".into(),
            "/repo/file.txt".into(),
        ];
        let snapshot = HashMap::from([("HEAD".into(), "other-head".into())]);
        let refs = for_command(&state, &cmd, &snapshot, Some(Path::new("/canonical/repo")));
        assert_eq!(
            refs.as_ref(),
            &HashMap::from([("HEAD".into(), "own-head".into())])
        );
        assert!(for_command(&state, &cmd, &snapshot, None).is_empty());
        state
            .worktrees
            .get_mut(Path::new("/canonical/repo"))
            .unwrap()
            .head = None;
        assert!(
            for_command(&state, &cmd, &snapshot, Some(Path::new("/canonical/repo"))).is_empty()
        );
    }

    #[test]
    fn other_commands_retain_snapshot_overlay_and_borrowing() {
        let mut state = family_state();
        state.refs.insert("HEAD".into(), "family-head".into());
        state
            .refs
            .insert("refs/heads/topic".into(), "topic-head".into());
        let cmd = normalized();
        assert!(matches!(
            for_command(&state, &cmd, &HashMap::new(), None),
            Cow::Borrowed(_)
        ));
        let refs = for_command(
            &state,
            &cmd,
            &HashMap::from([("HEAD".into(), "captured-head".into())]),
            None,
        );
        assert_eq!(refs.get("HEAD").unwrap(), "captured-head");
        assert_eq!(refs.get("refs/heads/topic").unwrap(), "topic-head");
        assert_eq!(state.refs.get("HEAD").unwrap(), "family-head");
    }
}
