//! HEAD contents for the files a checkpoint touches for the first time.

use crate::operations::git::fast_reader::FastObjectReader;
use crate::operations::git::repository::{Repository, batch_read_paths_at_treeishes};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Reads each file's content in `head_tree_id`, lossily decoded as UTF-8.
///
/// Loose objects are read straight from disk. Everything else (packed objects,
/// paths missing from HEAD) shares one batched Git lookup instead of costing
/// Git processes per file. A path that is not a blob in HEAD, or that could
/// not be read, is absent from the result; callers treat it as empty.
pub(super) fn read_head_contents<'a>(
    repo: &Repository,
    head_tree_id: Option<&str>,
    files: impl Iterator<Item = &'a String>,
) -> HashMap<String, Arc<str>> {
    let Some(tree_id) = head_tree_id else {
        return HashMap::new();
    };
    let reader = FastObjectReader::new(repo.common_dir());
    let mut contents = HashMap::new();
    let mut requests = Vec::new();
    for file in files {
        let loose = reader
            .try_tree_entry_for_path(tree_id, Path::new(file))
            .and_then(|oid| reader.try_read_blob(&oid));
        match loose {
            Some(bytes) => {
                contents.insert(file.clone(), Arc::from(String::from_utf8_lossy(&bytes)));
            }
            None => requests.push((tree_id.to_string(), file.clone())),
        }
    }
    match batch_read_paths_at_treeishes(repo, &requests) {
        Ok(batch) => contents.extend(
            batch
                .into_iter()
                .map(|((_, file), content)| (file, Arc::from(content))),
        ),
        Err(error) => tracing::debug!(%error, "checkpoint: batched HEAD read failed"),
    }
    contents
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::git::test_utils::TmpRepo;

    /// HEAD holds a text file, a nested file and a non-UTF-8 file; the request
    /// also names a path HEAD lacks and one that is a directory in HEAD.
    fn read(tmp: &TmpRepo, head_tree_id: Option<&str>) -> HashMap<String, Arc<str>> {
        let files = ["text.txt", "dir/nested.txt", "latin1.txt", "new.txt", "dir"];
        let files = files.map(String::from);
        read_head_contents(tmp.gitai_repo(), head_tree_id, files.iter())
    }

    #[test]
    fn loose_and_packed_head_objects_read_the_same_contents() {
        let tmp = TmpRepo::new().unwrap();
        tmp.write_file("text.txt", "one\ntwo\n", false).unwrap();
        tmp.write_file("dir/nested.txt", "nested\n", false).unwrap();
        std::fs::write(tmp.path().join("latin1.txt"), b"caf\xe9\n").unwrap();
        tmp.commit_all("base").unwrap();
        let tree = tmp.git_command(&["rev-parse", "HEAD^{tree}"]).unwrap();
        let expected = HashMap::from([
            ("text.txt".to_string(), Arc::<str>::from("one\ntwo\n")),
            ("dir/nested.txt".to_string(), Arc::from("nested\n")),
            ("latin1.txt".to_string(), Arc::from("caf\u{fffd}\n")),
        ]);

        assert_eq!(read(&tmp, Some(tree.trim())), expected, "loose objects");
        tmp.git_command(&["repack", "-a", "-d", "-q"]).unwrap();
        assert_eq!(read(&tmp, Some(tree.trim())), expected, "packed objects");
        assert!(read(&tmp, None).is_empty(), "no HEAD commit");
    }

    /// The batched lookup is newline-framed, but a tracked file may be named
    /// with a newline. Once HEAD is packed it must still read its contents:
    /// an empty baseline credits the AI with every line the file already had.
    #[cfg(not(windows))]
    #[test]
    fn packed_head_file_named_with_a_newline_keeps_its_contents() {
        let tmp = TmpRepo::new().unwrap();
        tmp.write_file("new\nline.txt", "one\ntwo\n", false)
            .unwrap();
        tmp.write_file("plain.txt", "plain\n", false).unwrap();
        tmp.commit_all("base").unwrap();
        tmp.git_command(&["repack", "-a", "-d", "-q"]).unwrap();
        let tree = tmp.git_command(&["rev-parse", "HEAD^{tree}"]).unwrap();
        let files = ["new\nline.txt", "plain.txt"].map(String::from);

        let contents = read_head_contents(tmp.gitai_repo(), Some(tree.trim()), files.iter());

        let expected = HashMap::from([
            ("new\nline.txt".to_string(), Arc::<str>::from("one\ntwo\n")),
            ("plain.txt".to_string(), Arc::from("plain\n")),
        ]);
        assert_eq!(contents, expected);
    }
}
