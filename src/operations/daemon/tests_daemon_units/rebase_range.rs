use super::*;
use crate::operations::authorship::rewrite::RebaseRange;

const OLD: &str = "1111111111111111111111111111111111111111";
const UPSTREAM: &str = "2222222222222222222222222222222222222222";

#[test]
fn rebase_boundary_uses_only_immutable_operation_evidence() {
    for (args, observed, expected) in [
        (vec![UPSTREAM], None, UPSTREAM.to_string()),
        (vec!["-i", "HEAD~66"], None, format!("{OLD}~66")),
        (vec!["@^2~3"], None, format!("{OLD}^2~3")),
        (vec!["main"], Some(UPSTREAM), UPSTREAM.to_string()),
        (
            vec!["--no-fork-point"],
            Some(UPSTREAM),
            UPSTREAM.to_string(),
        ),
    ] {
        let range = RebaseRange::from_command(
            &test_rebase_command(&args, vec![ref_change("HEAD", OLD, UPSTREAM)]),
            OLD,
            observed,
        );
        assert_eq!(range.old_base, Some(expected), "{args:?}");
    }
}

#[test]
fn named_rebase_does_not_promote_an_intermediate_head_to_old_boundary() {
    let intermediate = "3333333333333333333333333333333333333333";
    let command = test_rebase_command(
        &["main"],
        vec![
            ref_change("HEAD", OLD, UPSTREAM),
            ref_change("HEAD", UPSTREAM, intermediate),
        ],
    );
    assert_eq!(
        RebaseRange::from_command(&command, OLD, Some(intermediate)).old_base,
        None
    );
}

#[test]
fn rebase_boundary_rejects_mutable_fork_points_and_unparsed_options() {
    for args in [
        vec![],
        vec!["--root"],
        vec!["--fork-point", UPSTREAM],
        vec!["--fork-p", UPSTREAM],
        vec!["--keep-base", UPSTREAM],
        vec!["--onto", "new-base", "old-base"],
        vec!["--onto", "new-base", "HEAD@{1}"],
        vec!["--onto", "new-base", "HEAD~2", "feature"],
        vec!["--continue"],
    ] {
        let range =
            RebaseRange::from_command(&test_rebase_command(&args, vec![]), OLD, Some(UPSTREAM));
        assert_eq!(range.old_base, None, "{args:?}");
    }
    let mut pull = test_rebase_command(&["--rebase", "origin", "main"], vec![]);
    pull.primary_command = Some("pull".into());
    assert_eq!(
        RebaseRange::from_command(&pull, OLD, Some(UPSTREAM)).old_base,
        None
    );
}

#[test]
fn ambiguous_onto_discards_even_short_leading_groups() {
    for (args, discard) in [
        (vec!["--onto=new-base", "old-base"], true),
        (vec!["--fork-point", "main"], false),
        (vec![], false),
    ] {
        let range = RebaseRange::from_command(&test_rebase_command(&args, vec![]), OLD, None);
        assert_eq!(range.discard_untrusted_leading, discard);
    }
}
