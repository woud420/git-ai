use super::super::wire::Budget;
use super::{
    JjViewDecodeError, MAX_VIEW_COMMIT_REFERENCES, MAX_VIEW_NAME_BYTES, MAX_VIEW_WIRE_ENTRIES,
};
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

pub(super) type Target<'a> = Vec<Option<&'a [u8]>>;
pub(super) type NamedTargets<'a> = BTreeMap<&'a str, Target<'a>>;
pub(super) type RemoteRefs<'a> = BTreeMap<&'a str, RemoteRef<'a>>;

#[derive(PartialEq, Eq)]
pub(super) struct RemoteRef<'a> {
    pub target: Target<'a>,
    pub state: u32,
}

#[derive(Default)]
pub(super) struct RemoteView<'a> {
    pub bookmarks: RemoteRefs<'a>,
    pub tags: RemoteRefs<'a>,
}

#[derive(Default)]
pub(super) struct RawView<'a> {
    pub heads: BTreeSet<&'a [u8]>,
    pub local_bookmarks: NamedTargets<'a>,
    pub local_tags: NamedTargets<'a>,
    pub remotes: BTreeMap<&'a str, RemoteView<'a>>,
    pub git_refs: NamedTargets<'a>,
    pub git_heads: NamedTargets<'a>,
    pub workspaces: BTreeMap<&'a str, &'a [u8]>,
}

pub(super) struct Budgets {
    wire: Budget,
    names: Budget,
    commits: Budget,
    pub commit_references: usize,
}

impl Budgets {
    pub fn new() -> Self {
        Self {
            wire: Budget::new(MAX_VIEW_WIRE_ENTRIES, "wire entry limit exceeded"),
            names: Budget::new(MAX_VIEW_NAME_BYTES, "name byte limit exceeded"),
            commits: Budget::new(
                MAX_VIEW_COMMIT_REFERENCES,
                "commit reference limit exceeded",
            ),
            commit_references: 0,
        }
    }

    pub fn entry(&mut self) -> Result<(), JjViewDecodeError> {
        Ok(self.wire.take(1)?)
    }

    pub fn commit(&mut self) -> Result<(), JjViewDecodeError> {
        self.commits.take(1)?;
        self.commit_references += 1;
        Ok(())
    }

    pub fn name<'a>(&mut self, bytes: &'a [u8]) -> Result<&'a str, JjViewDecodeError> {
        self.names.take(bytes.len())?;
        std::str::from_utf8(bytes).map_err(|_| JjViewDecodeError("invalid utf8 name"))
    }
}

pub(super) fn insert<'a, T>(
    map: &mut BTreeMap<&'a str, T>,
    name: &'a str,
    value: T,
) -> Result<(), JjViewDecodeError> {
    match map.entry(name) {
        Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
        Entry::Occupied(_) => Err(JjViewDecodeError("duplicate map key")),
    }
}
