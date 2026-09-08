use super::*;

pub fn denied(case: &Case, config: &Config) {
    let mut journal = case.open();
    capture_current_state(&case.context(), deadline()).unwrap();
    reject_unchanged(case, &mut journal, config);
    assert_eq!(counts(case), [0; 4]);
    assert!(!case.namespace().exists());
}

pub fn sentinel_empty(case: &Case, config: &Config) {
    use git_ai::diagnostic_sentinels::{
        DEBUG_SELF_CHECK_REMOTE_URL, is_debug_self_check_remote_url,
    };

    assert!(!config.has_allowed_repositories());
    assert!(is_debug_self_check_remote_url(DEBUG_SELF_CHECK_REMOTE_URL));
    let context = case.context();
    assert!(!context.colocated);
    fs::write(
        context.git.common_dir.join("config"),
        format!("[remote \"self-check\"]\nurl = {DEBUG_SELF_CHECK_REMOTE_URL}\n"),
    )
    .unwrap();
    let captured = capture_current_state(&context, deadline()).unwrap();
    assert_eq!(captured.head_ids(), [MERGE_ID]);
    assert_eq!(captured.checkout().workspace_name, "default");
    let mut journal = case.open();
    let before = state(case);
    let result = case.register(&mut journal, config);
    assert!(
        state(case) == before,
        "sentinel register changed repository, config or SQL state"
    );
    error(result);
    error(case.reopen(&journal, config));
    assert!(
        state(case) == before,
        "sentinel reopen changed repository, config or SQL state"
    );
    assert_eq!(counts(case), [0; 4]);
    assert!(!case.namespace().exists());
}

pub fn remote(case: &Case, config: &Config, denied_policy: bool) {
    let context = case.context();
    assert!(!context.colocated);
    fs::write(
        context.git.common_dir.join("config"),
        format!("[remote \"fixture\"]\nurl = {REMOTE}\n"),
    )
    .unwrap();
    if denied_policy {
        denied(case, config);
    } else {
        behavior::install(case, config);
    }
}

pub fn canonical(case: &Case, config: &Config, denied_policy: bool) {
    fs::create_dir(case.root.join("allowed")).unwrap();
    let mut context = case.context();
    context.workspace_root = case.root.join("allowed/..");
    assert_ne!(context.workspace_root, case.root);
    assert_eq!(context.workspace_root.canonicalize().unwrap(), case.root);
    capture_current_state(&context, deadline()).unwrap();
    let mut journal = case.open();
    let before = state(case);
    let result = register_current_state(&mut journal, &context, config, deadline(), &mut budget());
    if denied_policy {
        error(result);
        assert_eq!(state(case), before);
        error(reopen_registered_current_state(
            &journal,
            &context,
            config,
            deadline(),
            &mut budget(),
        ));
        assert_eq!(state(case), before);
        assert!(!case.namespace().exists());
    } else {
        let registered = installed(result.unwrap());
        behavior::assert_registration(case, &journal, &registered, &[merge()], MERGE_ID, false);
    }
}

pub fn included(case: &Case, config: &Config, malformed: bool) {
    let context = case.context();
    let include = context.git.common_dir.join("excluded-policy.inc");
    fs::write(
        context.git.common_dir.join("config"),
        "[include]\npath = excluded-policy.inc\n",
    )
    .unwrap();
    fs::write(
        include,
        if malformed {
            "[remote \"unterminated\n".to_owned()
        } else {
            format!("[remote \"included\"]\nurl = {REMOTE}\n")
        },
    )
    .unwrap();
    denied(case, config);
}
