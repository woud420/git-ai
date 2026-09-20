use std::path::Path;

pub(super) fn proven_literal_worktree_path(
    global_args: &[String],
    worktree: &Path,
    path: &str,
) -> Option<String> {
    let mut root_cwd_proven = false;
    let mut literal = false;
    let mut globals = global_args.iter();
    while let Some(arg) = globals.next() {
        if arg == "--literal-pathspecs" {
            literal = true;
            continue;
        }
        let directory = if arg == "-C" {
            globals.next()?.as_str()
        } else {
            arg.strip_prefix("-C").filter(|value| !value.is_empty())?
        };
        root_cwd_proven = Path::new(directory).is_absolute() && Path::new(directory) == worktree;
    }
    let raw = Path::new(path);
    let relative = if raw.is_absolute() {
        raw.strip_prefix(worktree).ok()?.to_str()?
    } else if root_cwd_proven {
        path
    } else {
        return None;
    };
    // Environment settings can turn pathspec magic into a literal filename.
    // A plain path has no such interpretation change. Special names require
    // Git's explicit global literal mode, which rejects conflicting modes.
    if !literal && (path.contains(['*', '?', '[', ']']) || relative.contains(':')) {
        return None;
    }
    #[cfg(windows)]
    let relative = relative.replace('\\', "/");
    if relative.len() > 4096
        || relative.contains(['\n', '\r', '\0', '\\', '\u{fffd}'])
        || relative
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    Some(relative.to_string())
}
