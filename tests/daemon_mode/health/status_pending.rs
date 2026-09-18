use super::*;

pub(super) fn assert_checkpoint_status(repo: &TestRepo, pending: bool) {
    for diff_only in [false, true] {
        let mut args = vec!["status", "--json"];
        if diff_only {
            args.push("--diff-only");
        }
        let output = repo
            .git_ai_command_without_pre_sync_for_test(&args, &[])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let status: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            status["checkpoint_processing_pending"],
            json!(pending),
            "{status}"
        );
        let args = if diff_only {
            vec!["status", "--diff-only"]
        } else {
            vec!["status"]
        };
        let output = repo
            .git_ai_command_without_pre_sync_for_test(&args, &[])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            stderr.contains("Checkpoint processing is pending"),
            pending,
            "{stderr}"
        );
        if pending {
            assert!(!stderr.contains("install-hooks"), "{stderr}");
        }
    }
}
