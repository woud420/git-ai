use crate::model::domain::{NormalizedCommand, ResetKind};
use crate::model::git_oid::is_non_zero_oid;

pub(super) fn infer_reset_kind(args: &[String]) -> ResetKind {
    for (flag, kind) in [
        ("--soft", ResetKind::Soft),
        ("--mixed", ResetKind::Mixed),
        ("--hard", ResetKind::Hard),
        ("--merge", ResetKind::Merge),
        ("--keep", ResetKind::Keep),
    ] {
        if args.iter().any(|arg| arg == flag) {
            return kind;
        }
    }
    ResetKind::Mixed
}

pub(super) fn same_head_hard_reset(
    cmd: &NormalizedCommand,
    args: &[String],
) -> Option<(String, String)> {
    if cmd.exit_code != 0 || !matches!(infer_reset_kind(args), ResetKind::Hard) {
        return None;
    }
    cmd.ref_changes
        .iter()
        .find(|change| {
            change.reference == "HEAD" && change.old == change.new && is_non_zero_oid(&change.old)
        })
        .map(|change| (change.old.clone(), change.new.clone()))
}
