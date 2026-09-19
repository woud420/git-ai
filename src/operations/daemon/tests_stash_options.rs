use super::*;

#[test]
fn stash_options_preserve_flag_precedence_and_argument_boundaries() {
    for (args, paths, keep) in [
        (vec!["push", "-k"], vec![], true),
        (vec!["push", "-p"], vec![], true),
        (vec!["push", "--no-keep-index", "-p"], vec![], false),
        (vec!["push", "-p", "--no-keep-index"], vec![], false),
        (vec!["push", "--no-keep-index", "-k"], vec![], true),
        (
            vec!["push", "-km", "--no-keep-index", "--", "first.txt"],
            vec!["first.txt"],
            true,
        ),
        (vec!["push", "-mp", "first.txt"], vec!["first.txt"], false),
        (
            vec!["push", "--message=-p", "--", "--keep-index"],
            vec!["--keep-index"],
            false,
        ),
        (
            vec![
                "push",
                "--pathspec-file-nul",
                "-k",
                "--pathspec-from-file",
                "paths",
            ],
            vec![],
            true,
        ),
        (vec!["save", "-k", "save my changes"], vec![], true),
    ] {
        let mut command = test_rebase_command(&[], Vec::new());
        command.raw_argv = ["git", "stash"]
            .into_iter()
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect();
        command.primary_command = Some("stash".to_owned());
        command.invoked_command = Some("stash".to_owned());
        command.invoked_args = args.iter().map(|s| (*s).to_owned()).collect();
        let result = ActorDaemonCoordinator::stash_push_options_from_command(&command);
        assert_eq!(
            result,
            (paths.iter().map(|s| (*s).to_owned()).collect(), keep),
            "{args:?}"
        );
    }
}
