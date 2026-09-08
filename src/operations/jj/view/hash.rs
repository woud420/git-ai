use super::super::content_hash::{blob, count, hex};
use super::types::{NamedTargets, RawView, RemoteRefs, Target};
use blake2::{Blake2b512, Digest};

pub(super) fn view_id(view: &RawView<'_>) -> String {
    let mut hash = Blake2b512::new();
    count(&mut hash, view.heads.len());
    for head in &view.heads {
        blob(&mut hash, head);
    }
    named_targets(&mut hash, &view.local_bookmarks);
    named_targets(&mut hash, &view.local_tags);
    count(&mut hash, view.remotes.len());
    for (name, remote) in &view.remotes {
        blob(&mut hash, name.as_bytes());
        remote_refs(&mut hash, &remote.bookmarks);
        remote_refs(&mut hash, &remote.tags);
    }
    named_targets(&mut hash, &view.git_refs);
    named_targets(&mut hash, &view.git_heads);
    count(&mut hash, view.workspaces.len());
    for (name, commit) in &view.workspaces {
        blob(&mut hash, name.as_bytes());
        blob(&mut hash, commit);
    }
    hex(&hash.finalize())
}

fn named_targets(hash: &mut Blake2b512, values: &NamedTargets<'_>) {
    count(hash, values.len());
    for (name, value) in values {
        blob(hash, name.as_bytes());
        target(hash, value);
    }
}

fn remote_refs(hash: &mut Blake2b512, values: &RemoteRefs<'_>) {
    count(hash, values.len());
    for (name, value) in values {
        blob(hash, name.as_bytes());
        target(hash, &value.target);
        hash.update(value.state.to_le_bytes());
    }
}

fn target(hash: &mut Blake2b512, target: &Target<'_>) {
    count(hash, target.len());
    for value in target {
        match value {
            None => hash.update(0u32.to_le_bytes()),
            Some(commit) => {
                hash.update(1u32.to_le_bytes());
                blob(hash, commit);
            }
        }
    }
}
