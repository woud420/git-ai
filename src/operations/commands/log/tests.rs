use super::pager::*;
use super::*;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// -- double-dash separator --

#[test]
fn double_dash_stops_extraction() {
    let (global, rest) = extract_git_global_args(&s(&["--", "--bare"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["--", "--bare"]));
}

#[test]
fn double_dash_after_global_arg() {
    let (global, rest) = extract_git_global_args(&s(&["--paginate", "--", "--bare"]));
    assert_eq!(global, s(&["--paginate"]));
    assert_eq!(rest, s(&["--", "--bare"]));
}

#[test]
fn double_dash_alone() {
    let (global, rest) = extract_git_global_args(&s(&["--"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["--"]));
}

#[test]
fn double_dash_as_last_after_log_args() {
    let (global, rest) = extract_git_global_args(&s(&["--oneline", "--"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["--oneline", "--"]));
}

// -- no-value global flags --

#[test]
fn no_value_global_flags_extracted() {
    let (global, rest) = extract_git_global_args(&s(&["--paginate", "--oneline"]));
    assert_eq!(global, s(&["--paginate"]));
    assert_eq!(rest, s(&["--oneline"]));
}

#[test]
fn bare_flag_extracted() {
    let (global, rest) = extract_git_global_args(&s(&["--bare", "--graph"]));
    assert_eq!(global, s(&["--bare"]));
    assert_eq!(rest, s(&["--graph"]));
}

// -- takes-value global options --

#[test]
fn git_dir_spaced_form() {
    let (global, rest) = extract_git_global_args(&s(&["--git-dir", "/some/path", "--oneline"]));
    assert_eq!(global, s(&["--git-dir", "/some/path"]));
    assert_eq!(rest, s(&["--oneline"]));
}

#[test]
fn git_dir_equals_form() {
    let (global, rest) = extract_git_global_args(&s(&["--git-dir=/some/path", "--oneline"]));
    assert_eq!(global, s(&["--git-dir=/some/path"]));
    assert_eq!(rest, s(&["--oneline"]));
}

#[test]
fn takes_value_option_at_end_without_value() {
    let (global, rest) = extract_git_global_args(&s(&["--git-dir"]));
    assert_eq!(global, s(&["--git-dir"]));
    assert!(rest.is_empty());
}

// -- exec-path --

#[test]
fn exec_path_standalone() {
    let (global, rest) = extract_git_global_args(&s(&["--exec-path", "--oneline"]));
    assert_eq!(global, s(&["--exec-path"]));
    assert_eq!(rest, s(&["--oneline"]));
}

#[test]
fn exec_path_equals_form() {
    let (global, rest) = extract_git_global_args(&s(&["--exec-path=/usr/lib/git", "--graph"]));
    assert_eq!(global, s(&["--exec-path=/usr/lib/git"]));
    assert_eq!(rest, s(&["--graph"]));
}

// -- -c config override --

#[test]
fn dash_c_with_valid_config_key() {
    let (global, rest) = extract_git_global_args(&s(&["-c", "core.pager=cat", "--oneline"]));
    assert_eq!(global, s(&["-c", "core.pager=cat"]));
    assert_eq!(rest, s(&["--oneline"]));
}

#[test]
fn dash_c_without_dot_is_not_extracted() {
    // bare -c followed by something without section.key=val is git log's combined-diff
    let (global, rest) = extract_git_global_args(&s(&["-c", "foo=bar"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["-c", "foo=bar"]));
}

#[test]
fn dash_c_followed_by_log_option() {
    let (global, rest) = extract_git_global_args(&s(&["-c", "--format=%H"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["-c", "--format=%H"]));
}

#[test]
fn sticky_c_with_valid_config_key() {
    let (global, rest) = extract_git_global_args(&s(&["-ccore.pager=cat"]));
    assert_eq!(global, s(&["-ccore.pager=cat"]));
    assert!(rest.is_empty());
}

#[test]
fn sticky_c_without_dot_is_not_extracted() {
    // -cC=3 should NOT be extracted — no dot in key portion
    let (global, rest) = extract_git_global_args(&s(&["-cC=3"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["-cC=3"]));
}

// -- ambiguous short flags are NOT extracted --

#[test]
fn dash_capital_c_not_extracted() {
    let (global, rest) = extract_git_global_args(&s(&["-C", "--oneline"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["-C", "--oneline"]));
}

#[test]
fn dash_p_not_extracted() {
    let (global, rest) = extract_git_global_args(&s(&["-p"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["-p"]));
}

#[test]
fn dash_capital_p_not_extracted() {
    let (global, rest) = extract_git_global_args(&s(&["-P"]));
    assert!(global.is_empty());
    assert_eq!(rest, s(&["-P"]));
}

// -- empty args --

#[test]
fn empty_args() {
    let (global, rest) = extract_git_global_args(&s(&[]));
    assert!(global.is_empty());
    assert!(rest.is_empty());
}

// -- mixed scenarios --

#[test]
fn multiple_global_args_with_log_args() {
    let (global, rest) = extract_git_global_args(&s(&[
        "--paginate",
        "-c",
        "core.pager=less",
        "--oneline",
        "--graph",
    ]));
    assert_eq!(global, s(&["--paginate", "-c", "core.pager=less"]));
    assert_eq!(rest, s(&["--oneline", "--graph"]));
}

#[test]
fn global_args_then_double_dash_then_pathspecs() {
    let (global, rest) = extract_git_global_args(&s(&[
        "--no-pager",
        "--git-dir=/repo",
        "--oneline",
        "--",
        "src/",
        "--bare",
    ]));
    assert_eq!(global, s(&["--no-pager", "--git-dir=/repo"]));
    assert_eq!(rest, s(&["--oneline", "--", "src/", "--bare"]));
}

#[test]
fn log_raw_flag_is_consumed() {
    let parsed = parse_log_args(&s(&["--raw", "-n", "1"])).unwrap();
    assert!(parsed.show_raw_notes);
    assert_eq!(parsed.git_log_args, s(&["-n", "1"]));
}

#[test]
fn log_notes_flag_is_consumed() {
    let parsed = parse_log_args(&s(&["--notes", "--author=me"])).unwrap();
    assert!(parsed.show_raw_notes);
    assert_eq!(parsed.git_log_args, s(&["--author=me"]));
}

#[test]
fn log_show_notes_alias_is_consumed() {
    let parsed = parse_log_args(&s(&["--show-notes", "--author=me"])).unwrap();
    assert!(parsed.show_raw_notes);
    assert_eq!(parsed.git_log_args, s(&["--author=me"]));
}

#[test]
fn plain_mode_consumes_only_plain_flag() {
    let parsed = parse_log_args(&s(&["--plain", "--raw", "--format=%H", "--max-count=2"])).unwrap();
    assert!(parsed.plain);
    assert!(!parsed.show_raw_notes);
    assert_eq!(
        parsed.git_log_args,
        s(&["--raw", "--format=%H", "--max-count=2"])
    );
}

#[test]
fn plain_pathspec_after_double_dash_is_not_interpreted() {
    let parsed = parse_log_args(&s(&["--", "--plain"])).unwrap();
    assert!(!parsed.plain);
    assert_eq!(parsed.git_log_args, s(&["--", "--plain"]));
}

#[test]
fn plain_mode_allows_git_render_flags() {
    let parsed = parse_log_args(&s(&["--plain", "--graph", "--patch"])).unwrap();
    assert!(parsed.plain);
    assert_eq!(parsed.git_log_args, s(&["--graph", "--patch"]));
}

#[test]
fn oneline_is_consumed() {
    let parsed = parse_log_args(&s(&["--oneline", "--max-count=2"])).unwrap();
    assert!(parsed.oneline);
    assert_eq!(parsed.git_log_args, s(&["--max-count=2"]));
}

#[test]
fn unsupported_render_flag_errors() {
    let err = parse_log_args(&s(&["--graph"])).unwrap_err();
    assert!(err.contains("unsupported git log rendering option"));
}

#[test]
fn pathspec_after_double_dash_is_not_interpreted() {
    let parsed = parse_log_args(&s(&["--", "--graph"])).unwrap();
    assert_eq!(parsed.git_log_args, s(&["--", "--graph"]));
}

#[test]
fn pager_globals_are_not_kept_for_repository_commands() {
    assert_eq!(
        repository_global_args(&s(&["--paginate", "--no-pager", "--bare"])),
        s(&["--bare"])
    );
}

#[test]
fn truncate_for_width_preserves_ansi_escape_sequences() {
    let truncated = truncate_for_width("\x1b[90mabcdef\x1b[0m", 3);
    assert_eq!(truncated, "\x1b[90mabc\x1b[0m");
}

#[test]
fn truncate_for_width_does_not_cut_incomplete_ansi_reset() {
    let truncated = truncate_for_width("\x1b[90mabc\x1b[0mdef", 3);
    assert_eq!(truncated, "\x1b[90mabc\x1b[0m");
}
