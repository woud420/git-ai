use super::*;

#[test]
fn wltrace_records_working_log_read_append_and_gc() {
    let trace_dir = tempfile::tempdir().expect("create WLTRACE directory");
    let trace_path = trace_dir.path().join("working-log.trace");
    let trace_path_string = trace_path.to_string_lossy().into_owned();
    let repo = TestRepo::new_with_daemon_env(&[("GIT_AI_WLTRACE", &trace_path_string)]);

    let mut file = repo.filename("traced.txt");
    file.set_contents(lines!["AI content".ai()]);
    repo.stage_all_and_commit("Trace working-log lifecycle")
        .expect("traced commit should succeed");
    file.assert_committed_lines(lines!["AI content".ai()]);

    let trace = fs::read_to_string(&trace_path).expect("read WLTRACE output");
    for operation in [
        "op=working_log.read",
        "op=working_log.append",
        "op=working_log.gc.archive",
    ] {
        assert!(
            trace.contains(operation),
            "WLTRACE output missing {operation}:\n{trace}"
        );
    }
}
