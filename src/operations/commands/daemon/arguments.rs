use std::path::Path;

pub(super) fn parse_repo_arg(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--repo" && i + 1 < args.len() {
            return Some(args[i + 1].clone());
        }
        i += 1;
    }
    None
}

pub(super) fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

pub(super) fn default_repo_path() -> String {
    crate::operations::git::canonicalize::canonicalize_or_self(Path::new("."))
        .to_string_lossy()
        .to_string()
}

pub(super) fn is_help(value: &str) -> bool {
    value == "help" || value == "--help" || value == "-h"
}
