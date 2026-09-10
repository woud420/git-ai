use super::*;

pub fn install(case: &Case, config: &Config) {
    let context = case.context();
    assert!(!context.colocated);
    let remote = if case.name.ends_with("sentinel_install") {
        git_ai::diagnostic_sentinels::DEBUG_SELF_CHECK_REMOTE_URL
    } else {
        REMOTE
    };
    fs::write(
        context.git.common_dir.join("config"),
        format!("[remote \"history-fixture\"]\nurl = {remote}\n"),
    )
    .unwrap();
    let (journal, saved) = support::install(case, config);
    let result = collect(case, &journal, config);
    assert_result(&result, &saved, &[MERGE_ID], &[], &[MERGE_ID], false);
}

pub fn denied(case: &Case, config: &Config) {
    let journal = case.open();
    assert_eq!(counts(case), [1; 4]);
    let mut context = case.context();
    if case.name.ends_with("canonical_denied") {
        fs::create_dir(case.root.join("allowed")).unwrap();
        context.workspace_root = case.root.join("allowed/..");
        assert_ne!(context.workspace_root, case.root);
        assert_eq!(context.workspace_root.canonicalize().unwrap(), case.root);
    }
    let captured = capture_current_state(&context, deadline()).unwrap();
    assert_eq!(captured.head_ids(), [MERGE_ID]);
    failure(checked(
        case,
        &journal,
        &context,
        config,
        deadline(),
        &mut budget(),
    ));
}
