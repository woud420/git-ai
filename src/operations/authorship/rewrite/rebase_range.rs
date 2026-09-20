use crate::model::domain::NormalizedCommand;
use crate::operations::git::cli_parser::summarize_rebase_args;
use crate::operations::git::oid::is_non_zero_oid;

#[derive(Debug, Clone, Default)]
pub(crate) struct RebaseRange {
    pub old_base: Option<String>,
    pub discard_untrusted_leading: bool,
}

impl RebaseRange {
    pub(crate) fn from_command(
        command: &NormalizedCommand,
        original_head: &str,
        observed_onto: Option<&str>,
    ) -> Self {
        let args = &command.invoked_args;
        let summary = summarize_rebase_args(args);
        let mut range = Self {
            old_base: None,
            discard_untrusted_leading: summary.onto_spec.is_some(),
        };
        // Git accepts abbreviated options, while the shared argv summary
        // recognizes exact spellings. An unparsed --onto/--fork-point must not
        // turn a destination checkout into trusted old-range evidence.
        if args
            .iter()
            .any(|arg| arg.starts_with('-') && !known_option(arg))
        {
            range.discard_untrusted_leading = true;
            return range;
        }
        // Pull's observed checkout is post-fetch; Git can select its old fork
        // point from a pre-fetch reflog that this command does not capture.
        if command.primary_command.as_deref() != Some("rebase")
            || summary.is_control_mode
            || summary.has_root
            || args.iter().any(|arg| arg == "--fork-point")
            || (summary.positionals.is_empty() && !args.iter().any(|arg| arg == "--no-fork-point"))
        {
            return range;
        }
        if let Some(upstream) = summary.positionals.first() {
            if is_non_zero_oid(upstream) {
                range.old_base = Some(upstream.clone());
                return range;
            }
            // An explicit branch may have been checked out after argv's HEAD
            // was resolved. Only anchor HEAD-relative argv without that branch.
            if summary.positionals.len() == 1
                && is_non_zero_oid(original_head)
                && let Some(suffix) = upstream
                    .strip_prefix("HEAD")
                    .or_else(|| upstream.strip_prefix('@'))
                && is_parent_ancestry_suffix(suffix)
            {
                range.old_base = Some(format!("{original_head}{suffix}"));
                return range;
            }
        }
        // With --onto, the checkout identifies the destination, not the old
        // boundary; a mutable upstream name cannot recover that boundary later.
        if summary.onto_spec.is_none()
            && let Some(onto) = observed_onto.filter(|oid| is_non_zero_oid(oid))
            && command.ref_changes.iter().any(|change| {
                change.reference == "HEAD" && change.old == original_head && change.new == onto
            })
        {
            range.old_base = Some(onto.to_string());
        }
        range
    }
}

fn known_option(arg: &str) -> bool {
    matches!(
        arg,
        "--" | "-i"
            | "--interactive"
            | "-f"
            | "--force-rebase"
            | "--no-ff"
            | "--reapply-cherry-picks"
            | "--no-reapply-cherry-picks"
            | "--autosquash"
            | "--no-autosquash"
            | "--autostash"
            | "--no-autostash"
            | "--fork-point"
            | "--no-fork-point"
            | "--onto"
            | "--root"
            | "--update-refs"
            | "--no-update-refs"
            | "-r"
            | "--rebase-merges"
            | "--no-rebase-merges"
            | "-v"
            | "--verbose"
            | "-q"
            | "--quiet"
            | "--stat"
            | "--no-stat"
            | "-s"
            | "--strategy"
            | "-X"
            | "--strategy-option"
            | "-x"
            | "--exec"
            | "--empty"
            | "-C"
            | "-S"
            | "--gpg-sign"
    ) || [
        "--onto=",
        "--rebase-merges=",
        "--strategy=",
        "--strategy-option=",
        "--exec=",
        "--empty=",
        "--gpg-sign=",
    ]
    .iter()
    .any(|prefix| arg.starts_with(prefix))
}

fn is_parent_ancestry_suffix(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if !matches!(bytes[index], b'^' | b'~') {
            return false;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    true
}
