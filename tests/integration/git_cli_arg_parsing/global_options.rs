use super::{parse_git_cli_args, s};

#[test]
fn repeated_dash_c_and_dash_c_sticky() {
    let args = s(&[
        "-c",
        "user.name=alice",
        "-cuser.email=alice@example.com",
        "commit",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["-c", "user.name=alice", "-cuser.email=alice@example.com"])
    );
    assert_eq!(got.command, Some("commit".into()));
    assert!(got.command_args.is_empty());
}

#[test]
fn multiple_dash_c_and_casing_mixture() {
    let args = s(&[
        "-c",
        "core.filemode=false",
        "-cuser.name=alice",
        "-c",
        "user.email=alice@example.com",
        "status",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "-c",
            "core.filemode=false",
            "-cuser.name=alice",
            "-c",
            "user.email=alice@example.com"
        ])
    );
    assert_eq!(got.command, Some("status".into()));
    assert!(got.command_args.is_empty());
}

#[test]
fn repeated_dash_c_retained_order() {
    let args = s(&["-c", "a=1", "-c", "a=2", "rev-parse"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c", "a=1", "-c", "a=2"]));
    assert_eq!(got.command, Some("rev-parse".into()));
}

#[test]
fn dash_c_space_and_sticky_variants() {
    let args = s(&["-c", "name=val", "-cname2=val2", "log"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c", "name=val", "-cname2=val2"]));
    assert_eq!(got.command, Some("log".into()));
}

#[test]
fn config_env_equals_form() {
    let args = s(&[
        "--config-env",
        "http.proxy=HTTP_PROXY",
        "--config-env=core.askpass=GIT_ASKPASS",
        "fetch",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "--config-env",
            "http.proxy=HTTP_PROXY",
            "--config-env=core.askpass=GIT_ASKPASS"
        ])
    );
    assert_eq!(got.command, Some("fetch".into()));
}

#[test]
fn multiple_dash_c_with_command_args_present() {
    let args = s(&[
        "-c",
        "commit.gpgsign=true",
        "-c",
        "user.signingkey=ABC",
        "commit",
        "-m",
        "msg",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["-c", "commit.gpgsign=true", "-c", "user.signingkey=ABC"])
    );
    assert_eq!(got.command, Some("commit".into()));
    assert_eq!(got.command_args, s(&["-m", "msg"]));
}

#[test]
fn pathspec_toggles_as_globals() {
    let args = s(&[
        "--literal-pathspecs",
        "--noglob-pathspecs",
        "--icase-pathspecs",
        "ls-files",
        "-z",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "--literal-pathspecs",
            "--noglob-pathspecs",
            "--icase-pathspecs"
        ])
    );
    assert_eq!(got.command, Some("ls-files".into()));
    assert_eq!(got.command_args, s(&["-z"]));
}

#[test]
fn negated_pathspec_toggles_as_globals() {
    let args = s(&[
        "--no-literal-pathspecs",
        "--no-glob-pathspecs",
        "--no-noglob-pathspecs",
        "--no-icase-pathspecs",
        "ls-files",
        "-z",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "--no-literal-pathspecs",
            "--no-glob-pathspecs",
            "--no-noglob-pathspecs",
            "--no-icase-pathspecs"
        ])
    );
    assert_eq!(got.command, Some("ls-files".into()));
    assert_eq!(got.command_args, s(&["-z"]));
}

#[test]
fn fugitive_commit_with_no_literal_pathspecs() {
    // vim-fugitive passes --no-literal-pathspecs between -c flags when committing
    let args = s(&[
        "-c",
        "color.advice=false",
        "-c",
        "color.ui=false",
        "--no-literal-pathspecs",
        "-c",
        "advice.waitingForEditor=false",
        "commit",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "-c",
            "color.advice=false",
            "-c",
            "color.ui=false",
            "--no-literal-pathspecs",
            "-c",
            "advice.waitingForEditor=false"
        ])
    );
    assert_eq!(got.command, Some("commit".into()));
    assert!(got.command_args.is_empty());
}

#[test]
fn paginate_and_no_pager_both_present_kept_as_globals() {
    let args = s(&["--paginate", "--no-pager", "log"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["--paginate", "--no-pager"]));
    assert_eq!(got.command, Some("log".into()));
}

#[test]
fn multiple_dash_c_directives_before_end_of_options() {
    let args = s(&["-c", "a=b", "--", "commit", "-m", "x"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c", "a=b"]));
    assert_eq!(got.command, Some("commit".into()));
    assert_eq!(got.command_args, s(&["-m", "x"]));
}

#[test]
fn repeated_dash_c_and_multiple_dash_c_with_command_afterwards() {
    let args = s(&[
        "-c",
        "a=1",
        "-c",
        "b=2",
        "-c",
        "c=3",
        "rev-parse",
        "--is-inside-work-tree",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c", "a=1", "-c", "b=2", "-c", "c=3"]));
    assert_eq!(got.command, Some("rev-parse".into()));
    assert_eq!(got.command_args, s(&["--is-inside-work-tree"]));
}

#[test]
fn multiple_dash_c_and_dash_c_sticky_then_end_of_options_and_weird_command() {
    let args = s(&["-cfoo=bar", "-c", "a=b", "--", "--oddcmd", "arg"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-cfoo=bar", "-c", "a=b"]));
    assert_eq!(got.command, Some("--oddcmd".into()));
    assert_eq!(got.command_args, s(&["arg"]));
}

#[test]
fn dash_c_and_namespace_and_gitdir_and_worktree() {
    let args = s(&[
        "-c",
        "a=b",
        "--namespace=ns",
        "--git-dir",
        "/g",
        "--work-tree=/w",
        "status",
        "--porcelain",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "-c",
            "a=b",
            "--namespace=ns",
            "--git-dir",
            "/g",
            "--work-tree=/w"
        ])
    );
    assert_eq!(got.command, Some("status".into()));
    assert_eq!(got.command_args, s(&["--porcelain"]));
}

#[test]
fn list_cmds_as_global_takes_value() {
    let args = s(&["--list-cmds=main,others", "status"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["--list-cmds=main,others"]));
    assert_eq!(got.command, Some("status".into()));
}

#[test]
fn super_prefix_and_attr_source_globals() {
    let args = s(&[
        "--super-prefix=foo/",
        "--attr-source",
        "path/to/file",
        "check-attr",
        "crlf",
        "README",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["--super-prefix=foo/", "--attr-source", "path/to/file"])
    );
    assert_eq!(got.command, Some("check-attr".into()));
    assert_eq!(got.command_args, s(&["crlf", "README"]));
}

#[test]
fn multiple_dash_c_and_bare() {
    let args = s(&[
        "--bare",
        "-c",
        "init.defaultBranch=main",
        "rev-parse",
        "--git-dir",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["--bare", "-c", "init.defaultBranch=main"])
    );
    assert_eq!(got.command, Some("rev-parse".into()));
    assert_eq!(got.command_args, s(&["--git-dir"]));
}

#[test]
fn sticky_dash_c_then_command() {
    let args = s(&["-cfoo.bar=baz", "status"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-cfoo.bar=baz"]));
    assert_eq!(got.command, Some("status".into()));
}

#[test]
fn sticky_dash_c_then_end_of_options_then_command() {
    let args = s(&["-cfoo.bar=baz", "--", "status", "-s"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-cfoo.bar=baz"]));
    assert_eq!(got.command, Some("status".into()));
    assert_eq!(got.command_args, s(&["-s"]));
}

#[test]
fn sticky_dash_c_and_sticky_dash_c_with_equals_in_value() {
    let args = s(&["-chttp.extraHeader=Authorization: Bearer=XYZ", "fetch"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&["-chttp.extraHeader=Authorization: Bearer=XYZ"])
    );
    assert_eq!(got.command, Some("fetch".into()));
}

#[test]
fn dash_c_then_missing_value_at_end_is_kept_and_no_crash() {
    let args = s(&["-c"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c"])); // parser keeps it; validation is up to caller
    assert_eq!(got.command, None);
    assert!(got.command_args.is_empty());
}

#[test]
fn dash_c_value_but_no_command() {
    let args = s(&["-c", "a=b"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-c", "a=b"]));
    assert_eq!(got.command, None);
    assert!(got.command_args.is_empty());
}

#[test]
fn dash_c_and_cwd_changes_multiple_c_variants() {
    let args = s(&["-C", ".", "-C/tmp", "-C", "-", "status"]);
    let got = parse_git_cli_args(&args);
    assert_eq!(got.global_args, s(&["-C", ".", "-C/tmp", "-C", "-"]));
    assert_eq!(got.command, Some("status".into()));
}

#[test]
fn attr_source_and_super_prefix_mixed_with_namespace() {
    let args = s(&[
        "--attr-source=HEAD:/.gitattributes",
        "--super-prefix",
        "sub/",
        "--namespace",
        "foo",
        "check-attr",
        "eol",
        "a.txt",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "--attr-source=HEAD:/.gitattributes",
            "--super-prefix",
            "sub/",
            "--namespace",
            "foo"
        ])
    );
    assert_eq!(got.command, Some("check-attr".into()));
    assert_eq!(got.command_args, s(&["eol", "a.txt"]));
}

#[test]
fn bare_and_no_optional_locks_and_no_advice_and_no_lazy_fetch() {
    let args = s(&[
        "--bare",
        "--no-optional-locks",
        "--no-advice",
        "--no-lazy-fetch",
        "rev-parse",
        "HEAD",
    ]);
    let got = parse_git_cli_args(&args);
    assert_eq!(
        got.global_args,
        s(&[
            "--bare",
            "--no-optional-locks",
            "--no-advice",
            "--no-lazy-fetch"
        ])
    );
    assert_eq!(got.command, Some("rev-parse".into()));
    assert_eq!(got.command_args, s(&["HEAD"]));
}
