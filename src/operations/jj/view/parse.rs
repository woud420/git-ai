use super::super::wire::{Fields, boolean, identity, singular};
use super::JjViewDecodeError;
use super::entries;
use super::refs;
use super::types::{Budgets, RawView, RemoteRefs, insert};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn decode(bytes: &[u8]) -> Result<(RawView<'_>, usize), JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut view = RawView::default();
    let mut budgets = Budgets::new();
    let mut bookmark_names = BTreeSet::new();
    let mut legacy_remotes = BTreeMap::new();
    let mut legacy_wc = None;
    let mut legacy_head = None;
    let mut head_mirror = None;
    let mut migrated = None;
    while let Some(field) = fields.next(&[
        (1, 2),
        (2, 2),
        (3, 2),
        (5, 2),
        (6, 2),
        (7, 2),
        (8, 2),
        (9, 2),
        (11, 2),
        (12, 0),
        (13, 2),
    ])? {
        if matches!(field.tag, 3 | 5 | 6 | 8 | 11 | 13) {
            budgets.entry()?;
        }
        match field.tag {
            1 => {
                let head = identity(field.bytes()?, 20)?;
                budgets.commit()?;
                if !view.heads.insert(head) {
                    return Err(JjViewDecodeError("duplicate commit head"));
                }
            }
            2 => singular(&mut legacy_wc, field.bytes()?)?,
            3 => {
                let (name, target) = entries::named_target(field.bytes()?, &mut budgets, true)?;
                insert(&mut view.git_refs, name, target)?;
            }
            5 => {
                let bookmark = entries::bookmark(field.bytes()?, &mut budgets)?;
                if !bookmark_names.insert(bookmark.name) {
                    return Err(JjViewDecodeError("duplicate bookmark name"));
                }
                if bookmark.local.as_slice() != [None] {
                    insert(&mut view.local_bookmarks, bookmark.name, bookmark.local)?;
                }
                for (remote, target) in bookmark.remotes {
                    insert(
                        legacy_remotes.entry(remote).or_default(),
                        bookmark.name,
                        target,
                    )?;
                }
            }
            6 => {
                let (name, target) = entries::named_target(field.bytes()?, &mut budgets, false)?;
                insert(&mut view.local_tags, name, target)?;
            }
            7 => singular(&mut legacy_head, field.bytes()?)?,
            8 => {
                let (name, commit) = entries::workspace(field.bytes()?, &mut budgets)?;
                insert(&mut view.workspaces, name, commit)?;
            }
            9 => singular(&mut head_mirror, field.bytes()?)?,
            11 => {
                let (name, remote) = entries::remote_view(field.bytes()?, &mut budgets)?;
                insert(&mut view.remotes, name, remote)?;
            }
            12 => singular(&mut migrated, boolean(field.varint()?)?)?,
            13 => {
                let (name, target) = entries::named_target(field.bytes()?, &mut budgets, false)?;
                insert(&mut view.git_heads, name, target)?;
            }
            _ => return Err(JjViewDecodeError("unknown view field")),
        }
    }
    if view.heads.is_empty() {
        return Err(JjViewDecodeError("view must contain a commit head"));
    }
    if !legacy_wc.unwrap_or_default().is_empty()
        || !legacy_head.unwrap_or_default().is_empty()
        || !migrated.unwrap_or_default()
    {
        return Err(JjViewDecodeError("unsupported legacy view profile"));
    }
    check_remote_mirrors(&view, &legacy_remotes)?;
    if let Some(raw) = head_mirror {
        let mirror = refs::target(Some(raw), &mut budgets, false)?;
        if view.git_heads.get("default") != Some(&mirror) {
            return Err(JjViewDecodeError("inconsistent Git HEAD mirror"));
        }
    }
    Ok((view, budgets.commit_references))
}

fn check_remote_mirrors(
    view: &RawView<'_>,
    legacy: &BTreeMap<&str, RemoteRefs<'_>>,
) -> Result<(), JjViewDecodeError> {
    if view.remotes.is_empty() {
        return if legacy.is_empty() {
            Ok(())
        } else {
            Err(JjViewDecodeError("unsupported legacy remote-only view"))
        };
    }
    for (name, refs) in legacy {
        if view.remotes.get(name).map(|remote| &remote.bookmarks) != Some(refs) {
            return Err(JjViewDecodeError("inconsistent legacy remote mirror"));
        }
    }
    for (name, remote) in &view.remotes {
        if !remote.bookmarks.is_empty() && legacy.get(name) != Some(&remote.bookmarks) {
            return Err(JjViewDecodeError("inconsistent legacy remote mirror"));
        }
    }
    Ok(())
}
