use super::side_effect_helpers::parsed_invocation_for_normalized_command;
use crate::model::domain::NormalizedCommand;
use crate::operations::git::oid::is_non_zero_oid;

pub(super) fn is_explicit_path_checkout(cmd: &NormalizedCommand) -> bool {
    let parsed = parsed_invocation_for_normalized_command(cmd);
    parsed.command.as_deref() == Some("checkout")
        && matches!(parsed.command_args.as_slice(), [source, separator, _]
            if is_non_zero_oid(source) && separator == "--")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::operations::daemon::tests_fixtures::normalized_command;

    #[test]
    fn explicit_path_checkout_is_a_commit_a_separator_and_one_path() {
        let head = "a".repeat(40);
        let is_explicit = |args: &[&str]| {
            let argv = [&["git", "checkout"], args].concat();
            is_explicit_path_checkout(&normalized_command("checkout", &argv))
        };

        assert!(is_explicit(&[&head, "--", "file.txt"]));
        for args in [
            vec![head.as_str(), "file.txt"],
            vec![&head, "other.txt", "file.txt"],
            vec!["--", "file.txt"],
            vec!["--ours", "--", "file.txt"],
            vec!["--theirs", "--", "file.txt"],
            vec!["--patch", &head, "--", "file.txt"],
            vec!["--force", &head, "--", "file.txt"],
            vec![&head, "--pathspec-from-file=paths"],
        ] {
            assert!(!is_explicit(&args), "{args:?}");
        }
    }
}
