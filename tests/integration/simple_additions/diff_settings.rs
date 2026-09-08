use super::run_simple_additions_with_diff_settings;

#[test]
fn test_simple_additions_with_base_commit_and_custom_diff_config() {
    run_simple_additions_with_diff_settings(&[
        ("diff.wordregex", r"\w+|[^[:space:]]+"),
        ("diff.mnemonicprefix", "true"),
        ("diff.renames", "copies"),
        ("diff.noprefix", "true"),
    ]);
}

#[test]
fn test_simple_additions_with_diff_noprefix_enabled() {
    run_simple_additions_with_diff_settings(&[("diff.noprefix", "true")]);
}

#[test]
fn test_simple_additions_with_diff_mnemonicprefix_enabled() {
    run_simple_additions_with_diff_settings(&[("diff.mnemonicprefix", "true")]);
}

#[test]
fn test_simple_additions_with_diff_renames_copies() {
    run_simple_additions_with_diff_settings(&[("diff.renames", "copies")]);
}

#[test]
fn test_simple_additions_with_diff_relative_enabled() {
    run_simple_additions_with_diff_settings(&[("diff.relative", "true")]);
}

#[test]
fn test_simple_additions_with_custom_diff_prefixes() {
    run_simple_additions_with_diff_settings(&[
        ("diff.srcPrefix", "SRC/"),
        ("diff.dstPrefix", "DST/"),
    ]);
}

#[test]
fn test_simple_additions_with_diff_algorithm_histogram() {
    run_simple_additions_with_diff_settings(&[("diff.algorithm", "histogram")]);
}

#[test]
fn test_simple_additions_with_diff_indent_heuristic_disabled() {
    run_simple_additions_with_diff_settings(&[("diff.indentHeuristic", "false")]);
}

#[test]
fn test_simple_additions_with_diff_inter_hunk_context() {
    run_simple_additions_with_diff_settings(&[("diff.interHunkContext", "8")]);
}

#[test]
fn test_simple_additions_with_color_diff_always() {
    run_simple_additions_with_diff_settings(&[("color.diff", "always"), ("color.ui", "always")]);
}
