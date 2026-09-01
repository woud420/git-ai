/// Normalize a revision token that begins with a user-typed lowercase `head`.
///
/// On case-insensitive filesystems, Git can resolve `head` through the common
/// Git directory instead of a linked worktree's `HEAD`. Only supported HEAD
/// suffixes are preserved so branch names such as `head@topic` stay intact.
pub(crate) fn normalize_head_rev(rev: &str) -> String {
    if rev
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("head"))
    {
        let suffix = &rev[4..];
        if suffix.is_empty()
            || suffix.starts_with('~')
            || suffix.starts_with('^')
            || suffix.starts_with("@{")
        {
            return format!("HEAD{suffix}");
        }
    }

    rev.to_string()
}

#[cfg(test)]
mod tests {
    use super::normalize_head_rev;

    #[test]
    fn normalizes_supported_head_forms() {
        for (revision, expected) in [
            ("head", "HEAD"),
            ("HeAd~2", "HEAD~2"),
            ("head^1", "HEAD^1"),
            ("head@{0}", "HEAD@{0}"),
        ] {
            assert_eq!(normalize_head_rev(revision), expected);
        }
    }

    #[test]
    fn preserves_other_revision_names() {
        for revision in ["header", "head@topic", "中文分支"] {
            assert_eq!(normalize_head_rev(revision), revision);
        }
    }
}
