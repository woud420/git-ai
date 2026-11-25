use flate2::read::ZlibDecoder;
use std::fs;
use std::io::Read;
use std::path::Path;

use super::oid::is_full_oid;

fn oid_byte_len(oid: &str) -> usize {
    oid.len() / 2
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadKind {
    Symbolic(String),
    Detached(String),
}

/// Fast worktree-aware ref resolution by reading .git/ files directly.
///
/// Handles loose refs, packed-refs, and symbolic refs (one level of indirection).
/// Returns None when the fast path cannot resolve (caller falls back to git CLI).
pub struct FastRefReader<'a> {
    git_dir: &'a Path,
    common_dir: &'a Path,
}

impl<'a> FastRefReader<'a> {
    pub fn new(git_dir: &'a Path, common_dir: &'a Path) -> Self {
        Self {
            git_dir,
            common_dir,
        }
    }

    /// Read HEAD: `Some(Symbolic(..))` for a symbolic HEAD, `Some(Detached(..))`
    /// for a detached one, or `None` if HEAD can't be read, has an unexpected
    /// format, or is the `refs/heads/.invalid` stub of a reftable repository
    /// (the real HEAD lives in the reftable, so callers fall back to git).
    pub fn try_read_head(&self) -> Option<HeadKind> {
        let head_path = self.git_dir.join("HEAD");
        let content = fs::read_to_string(&head_path).ok()?;
        let trimmed = content.trim();

        if let Some(refname) = trimmed.strip_prefix("ref: ") {
            let refname = refname.trim();
            if !refname.is_empty() && refname != "refs/heads/.invalid" {
                return Some(HeadKind::Symbolic(refname.to_string()));
            }
        }

        if is_full_oid(trimmed) {
            return Some(HeadKind::Detached(trimmed.to_string()));
        }

        None
    }

    /// Resolve a refname (e.g., "refs/heads/main") to its OID.
    ///
    /// Checks loose refs in both common_dir and git_dir, then packed-refs.
    /// Handles one level of symbolic ref indirection.
    /// Returns None if the ref cannot be resolved via filesystem.
    pub fn try_resolve_ref(&self, refname: &str) -> Option<String> {
        if refname == "HEAD" {
            match self.try_read_head()? {
                HeadKind::Detached(oid) => return Some(oid),
                HeadKind::Symbolic(target) => return self.try_resolve_ref(&target),
            }
        }

        // Check loose refs: common_dir first, then git_dir
        for base in [self.common_dir, self.git_dir] {
            let path = base.join(refname);
            if let Ok(contents) = fs::read_to_string(&path) {
                let candidate = contents.trim();
                if is_full_oid(candidate) {
                    return Some(candidate.to_string());
                }
                // One level of symbolic ref indirection
                if let Some(target) = candidate.strip_prefix("ref: ") {
                    let target = target.trim();
                    return self.resolve_without_recursion(target);
                }
            }
        }

        // Check packed-refs in common_dir
        self.try_packed_ref(refname)
    }

    fn resolve_without_recursion(&self, refname: &str) -> Option<String> {
        for base in [self.common_dir, self.git_dir] {
            let path = base.join(refname);
            if let Ok(contents) = fs::read_to_string(&path) {
                let candidate = contents.trim();
                if is_full_oid(candidate) {
                    return Some(candidate.to_string());
                }
            }
        }
        self.try_packed_ref(refname)
    }

    fn try_packed_ref(&self, refname: &str) -> Option<String> {
        let packed_refs_path = self.common_dir.join("packed-refs");
        let contents = fs::read_to_string(packed_refs_path).ok()?;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut parts = line.split_whitespace();
            let oid = parts.next()?;
            let name = parts.next()?;
            if name == refname && is_full_oid(oid) {
                return Some(oid.to_string());
            }
        }
        None
    }
}

/// Fast loose object reading by directly parsing .git/objects/ files.
///
/// Only handles loose objects (not packfiles). Returns None for packed objects,
/// allowing the caller to fall back to git CLI.
pub struct FastObjectReader<'a> {
    common_dir: &'a Path,
}

impl<'a> FastObjectReader<'a> {
    pub fn new(common_dir: &'a Path) -> Self {
        Self { common_dir }
    }

    fn has_alternates(&self) -> bool {
        self.common_dir
            .join("objects")
            .join("info")
            .join("alternates")
            .exists()
    }

    fn object_path(&self, oid: &str) -> Option<std::path::PathBuf> {
        if !is_full_oid(oid) {
            return None;
        }
        Some(
            self.common_dir
                .join("objects")
                .join(&oid[..2])
                .join(&oid[2..]),
        )
    }

    fn decompress_object(&self, oid: &str) -> Option<Vec<u8>> {
        if self.has_alternates() {
            return None;
        }
        let path = self.object_path(oid)?;
        let compressed = fs::read(&path).ok()?;
        let mut decoder = ZlibDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).ok()?;
        Some(decompressed)
    }

    /// Read just the type from a loose object header without fully decompressing the content.
    pub fn try_read_object_type(&self, oid: &str) -> Option<String> {
        let data = self.decompress_object(oid)?;
        let null_pos = data.iter().position(|&b| b == 0)?;
        let header = std::str::from_utf8(&data[..null_pos]).ok()?;
        let type_str = header.split(' ').next()?;
        Some(type_str.to_string())
    }

    /// Read a loose blob object's content.
    ///
    /// Returns None if the object doesn't exist (packed), isn't a blob, or can't be read.
    pub fn try_read_blob(&self, oid: &str) -> Option<Vec<u8>> {
        let data = self.decompress_object(oid)?;
        let null_pos = data.iter().position(|&b| b == 0)?;
        let header = std::str::from_utf8(&data[..null_pos]).ok()?;
        if !header.starts_with("blob ") {
            return None;
        }
        Some(data[null_pos + 1..].to_vec())
    }

    /// Read a loose commit object and extract its tree OID.
    ///
    /// Commit format after header: `tree {hex-oid}\n...`
    pub fn try_read_commit_tree_oid(&self, commit_oid: &str) -> Option<String> {
        let data = self.decompress_object(commit_oid)?;
        let null_pos = data.iter().position(|&b| b == 0)?;
        let header = std::str::from_utf8(&data[..null_pos]).ok()?;
        if !header.starts_with("commit ") {
            return None;
        }
        let body = std::str::from_utf8(&data[null_pos + 1..]).ok()?;
        let first_line = body.lines().next()?;
        let tree_oid = first_line.strip_prefix("tree ")?;
        let tree_oid = tree_oid.trim();
        if is_full_oid(tree_oid) {
            Some(tree_oid.to_string())
        } else {
            None
        }
    }

    /// Read a loose commit object's parent OIDs, in order.
    ///
    /// Returns None when git could report other parents than the object
    /// records: in a shallow clone, with grafts, or with replace refs.
    pub fn try_read_commit_parents(&self, commit_oid: &str) -> Option<Vec<String>> {
        if self.parents_may_be_rewritten() {
            return None;
        }
        let data = self.decompress_object(commit_oid)?;
        let null_pos = data.iter().position(|&b| b == 0)?;
        if !data.starts_with(b"commit ") {
            return None;
        }
        let mut lines = data[null_pos + 1..].split(|&b| b == b'\n');
        lines.next()?.strip_prefix(b"tree ")?;
        let mut parents = Vec::new();
        for line in lines {
            let Some(oid) = line.strip_prefix(b"parent ") else {
                break;
            };
            let oid = std::str::from_utf8(oid).ok()?;
            if !is_full_oid(oid) {
                return None;
            }
            parents.push(oid.to_string());
        }
        Some(parents)
    }

    fn parents_may_be_rewritten(&self) -> bool {
        let dir = self.common_dir;
        self.objects_may_be_replaced()
            || dir.join("shallow").exists()
            || dir.join("info").join("grafts").exists()
    }

    /// Whether Git could read other objects than the loose files hold: a
    /// replace ref, or refs this reader can't list.
    fn objects_may_be_replaced(&self) -> bool {
        let dir = self.common_dir;
        std::env::var_os("GIT_REPLACE_REF_BASE").is_some()
            || dir.join("reftable").exists()
            || fs::read_dir(dir.join("refs").join("replace"))
                .is_ok_and(|mut entries| entries.next().is_some())
            || fs::read_to_string(dir.join("packed-refs"))
                .is_ok_and(|packed| packed.contains(" refs/replace/"))
    }

    /// A loose object's type and content.
    fn read_loose(&self, oid: &str) -> Option<(String, Vec<u8>)> {
        let mut data = self.decompress_object(oid)?;
        let null_pos = data.iter().position(|&b| b == 0)?;
        let kind = std::str::from_utf8(&data[..null_pos])
            .ok()?
            .split(' ')
            .next()?;
        let kind = kind.to_string();
        Some((kind, data.split_off(null_pos + 1)))
    }

    /// The blob that `git cat-file --batch` prints for `<treeish>:<path>`,
    /// where `treeish` is a full commit or tree OID, or a commit OID followed
    /// by `^` for its first parent, read from loose objects.
    ///
    /// `Some(None)` when Git reports no blob there: no such entry, a
    /// directory or submodule, or a root commit's missing parent. `None` when only Git can tell: a packed or
    /// absent object, alternates, replace refs, an object directory set in
    /// the environment, or a path that isn't plain `a/b/c`.
    pub fn try_read_blob_at_path(&self, treeish: &str, path: &str) -> Option<Option<Vec<u8>>> {
        if self.objects_may_be_replaced()
            || std::env::var_os("GIT_OBJECT_DIRECTORY").is_some()
            || std::env::var_os("GIT_ALTERNATE_OBJECT_DIRECTORIES").is_some()
        {
            return None;
        }
        let names: Vec<&str> = path.split('/').collect();
        if names.iter().any(|name| matches!(*name, "" | "." | "..")) {
            return None;
        }
        let treeish = match treeish.strip_suffix('^') {
            Some(child) => match self.try_read_commit_parents(child)?.into_iter().next() {
                Some(parent) => parent,
                None => return Some(None),
            },
            None => treeish.to_string(),
        };
        let mut tree_oid = match self.read_loose(&treeish)? {
            (kind, _) if kind == "tree" => treeish,
            (kind, _) if kind == "commit" => self.try_read_commit_tree_oid(&treeish)?,
            _ => return None,
        };
        for (depth, name) in names.iter().enumerate() {
            let (kind, entries) = self.read_loose(&tree_oid)?;
            if kind != "tree" {
                return None;
            }
            let Some((mode, oid)) = tree_entry_lookup(&entries, name, oid_byte_len(&tree_oid))?
            else {
                return Some(None);
            };
            let is_dir = mode == DIRECTORY_MODE;
            if depth + 1 < names.len() {
                if !is_dir {
                    return Some(None);
                }
                tree_oid = oid;
            } else if is_dir || mode == GITLINK_MODE {
                return Some(None);
            } else {
                let (kind, content) = self.read_loose(&oid)?;
                return (kind == "blob").then_some(Some(content));
            }
        }
        None
    }

    /// Traverse a tree (and subtrees) to find the blob OID at the given path.
    ///
    /// For "src/main.rs", reads the root tree, finds "src" subtree, reads it,
    /// then finds "main.rs" blob entry.
    ///
    /// Returns None if any tree along the path is packed or the path doesn't exist.
    pub fn try_tree_entry_for_path(&self, tree_oid: &str, path: &Path) -> Option<String> {
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();

        if components.is_empty() {
            return None;
        }

        let mut current_tree_oid = tree_oid.to_string();

        for (i, component) in components.iter().enumerate() {
            let is_last = i == components.len() - 1;
            let entry_oid = self.find_tree_entry(&current_tree_oid, component)?;

            if is_last {
                return Some(entry_oid);
            }
            // Intermediate component must be a subtree
            current_tree_oid = entry_oid;
        }

        None
    }

    /// Find a named entry in a tree object, returning its OID.
    fn find_tree_entry(&self, tree_oid: &str, name: &str) -> Option<String> {
        let data = self.decompress_object(tree_oid)?;
        let null_pos = data.iter().position(|&b| b == 0)?;
        let header = std::str::from_utf8(&data[..null_pos]).ok()?;
        if !header.starts_with("tree ") {
            return None;
        }

        let hash_len = oid_byte_len(tree_oid);
        let entries_data = &data[null_pos + 1..];
        tree_entry(entries_data, name, hash_len).map(|(_mode, oid)| oid)
    }
}

/// Mode of a tree entry that is itself a tree.
pub(crate) const DIRECTORY_MODE: u32 = 0o40000;

/// Mode of a submodule entry.
const GITLINK_MODE: u32 = 0o160000;

/// The mode and OID of the entry called `name` in a tree object's raw entries.
///
/// Tree entry format: `{mode} {name}\0{raw-binary-hash}`
pub(crate) fn tree_entry(data: &[u8], name: &str, hash_len: usize) -> Option<(u32, String)> {
    tree_entry_lookup(data, name, hash_len).flatten()
}

/// Like [`tree_entry`], telling a missing entry (`Some(None)`) apart from
/// entries it can't parse (`None`).
fn tree_entry_lookup(
    mut data: &[u8],
    name: &str,
    hash_len: usize,
) -> Option<Option<(u32, String)>> {
    while !data.is_empty() {
        // Find the space separating mode from name
        let space_pos = data.iter().position(|&b| b == b' ')?;
        // Find the null byte after name
        let null_pos = data[space_pos + 1..].iter().position(|&b| b == 0)?;
        let null_pos = space_pos + 1 + null_pos;

        // The hash follows the null byte
        let hash_start = null_pos + 1;
        if data.len() < hash_start + hash_len {
            return None;
        }
        let hash_bytes = &data[hash_start..hash_start + hash_len];

        if &data[space_pos + 1..null_pos] == name.as_bytes() {
            let mode = std::str::from_utf8(&data[..space_pos]).ok()?;
            let oid = hash_bytes
                .iter()
                .map(|b| format!("{:02x}", b))
                .collect::<String>();
            return Some(Some((u32::from_str_radix(mode, 8).ok()?, oid)));
        }

        data = &data[hash_start + hash_len..];
    }
    Some(None)
}

#[cfg(test)]
#[path = "fast_reader_tests.rs"]
mod tests;
