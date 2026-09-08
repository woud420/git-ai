use super::super::wire::{Fields, identity, singular};
use super::JjViewDecodeError;
use super::refs;
use super::types::{Budgets, RemoteRef, RemoteRefs, RemoteView, Target, insert};

pub(super) struct Bookmark<'a> {
    pub name: &'a str,
    pub local: Target<'a>,
    pub remotes: RemoteRefs<'a>,
}

pub(super) fn bookmark<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
) -> Result<Bookmark<'a>, JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut name = None;
    let mut target = None;
    let mut remotes = RemoteRefs::new();
    while let Some(field) = fields.next(&[(1, 2), (2, 2), (3, 2)])? {
        match field.tag {
            1 => singular(&mut name, budgets.name(field.bytes()?)?)?,
            2 => singular(&mut target, field.bytes()?)?,
            3 => {
                budgets.entry()?;
                let (remote, value) = legacy_remote(field.bytes()?, budgets)?;
                insert(&mut remotes, remote, value)?;
            }
            _ => return Err(JjViewDecodeError("unknown bookmark field")),
        }
    }
    Ok(Bookmark {
        name: name.unwrap_or_default(),
        local: refs::target(target, budgets, true)?,
        remotes,
    })
}

fn legacy_remote<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
) -> Result<(&'a str, RemoteRef<'a>), JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut name = None;
    let mut target = None;
    let mut state = None;
    while let Some(field) = fields.next(&[(1, 2), (2, 2), (3, 0)])? {
        match field.tag {
            1 => singular(&mut name, budgets.name(field.bytes()?)?)?,
            2 => singular(&mut target, field.bytes()?)?,
            3 => singular(&mut state, refs::state(field.varint()?)?)?,
            _ => return Err(JjViewDecodeError("unknown legacy remote field")),
        }
    }
    Ok((
        name.unwrap_or_default(),
        RemoteRef {
            target: refs::target(target, budgets, false)?,
            state: state.unwrap_or_default(),
        },
    ))
}

pub(super) fn named_target<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
    git_ref: bool,
) -> Result<(&'a str, Target<'a>), JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let allowed: &[(u64, u8)] = if git_ref {
        &[(1, 2), (2, 2), (3, 2)]
    } else {
        &[(1, 2), (2, 2)]
    };
    let mut name = None;
    let mut target = None;
    let mut legacy = None;
    while let Some(field) = fields.next(allowed)? {
        match field.tag {
            1 => singular(&mut name, budgets.name(field.bytes()?)?)?,
            2 if git_ref => singular(&mut legacy, field.bytes()?)?,
            2 | 3 => singular(&mut target, field.bytes()?)?,
            _ => return Err(JjViewDecodeError("unknown named target field")),
        }
    }
    if git_ref && (!legacy.unwrap_or_default().is_empty() || target.is_none()) {
        return Err(JjViewDecodeError("unsupported legacy GitRef"));
    }
    Ok((
        name.unwrap_or_default(),
        refs::target(target, budgets, true)?,
    ))
}

pub(super) fn workspace<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
) -> Result<(&'a str, &'a [u8]), JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut name = None;
    let mut commit = None;
    while let Some(field) = fields.next(&[(1, 2), (2, 2)])? {
        match field.tag {
            1 => singular(&mut name, budgets.name(field.bytes()?)?)?,
            2 => singular(&mut commit, identity(field.bytes()?, 20)?)?,
            _ => return Err(JjViewDecodeError("unknown workspace field")),
        }
    }
    let commit = commit.ok_or(JjViewDecodeError("missing workspace commit identity"))?;
    budgets.commit()?;
    Ok((name.unwrap_or_default(), commit))
}

pub(super) fn remote_view<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
) -> Result<(&'a str, RemoteView<'a>), JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut name = None;
    let mut remote = RemoteView::default();
    while let Some(field) = fields.next(&[(1, 2), (2, 2), (3, 2)])? {
        match field.tag {
            1 => singular(&mut name, budgets.name(field.bytes()?)?)?,
            tag @ (2 | 3) => {
                budgets.entry()?;
                let (name, value) = remote_ref(field.bytes()?, budgets)?;
                let map = if tag == 2 {
                    &mut remote.bookmarks
                } else {
                    &mut remote.tags
                };
                insert(map, name, value)?;
            }
            _ => return Err(JjViewDecodeError("unknown remote view field")),
        }
    }
    Ok((name.unwrap_or_default(), remote))
}

fn remote_ref<'a>(
    bytes: &'a [u8],
    budgets: &mut Budgets,
) -> Result<(&'a str, RemoteRef<'a>), JjViewDecodeError> {
    let mut fields = Fields::new(bytes);
    let mut name = None;
    let mut target = Vec::new();
    let mut state = None;
    while let Some(field) = fields.next(&[(1, 2), (2, 2), (3, 0)])? {
        match field.tag {
            1 => singular(&mut name, budgets.name(field.bytes()?)?)?,
            2 => {
                budgets.entry()?;
                target.push(refs::term(field.bytes()?, budgets, true)?);
            }
            3 => singular(&mut state, refs::state(field.varint()?)?)?,
            _ => return Err(JjViewDecodeError("unknown remote ref field")),
        }
    }
    if target.len() % 2 != 1 {
        return Err(JjViewDecodeError("invalid remote target term count"));
    }
    Ok((
        name.unwrap_or_default(),
        RemoteRef {
            target,
            state: state.unwrap_or_default(),
        },
    ))
}
