use super::side_effect_helpers::parsed_invocation_for_normalized_command;
use crate::model::domain::NormalizedCommand;
use crate::operations::git::oid::is_non_zero_oid;

pub(super) fn is_explicit_path_checkout(cmd: &NormalizedCommand) -> bool {
    let parsed = parsed_invocation_for_normalized_command(cmd);
    parsed.command.as_deref() == Some("checkout")
        && matches!(parsed.command_args.as_slice(), [source, separator, _]
            if is_non_zero_oid(source) && separator == "--")
}
