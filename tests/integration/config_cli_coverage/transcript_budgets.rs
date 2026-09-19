use super::*;

#[test]
fn ingestion_byte_budgets_round_trip_and_environment_takes_precedence() {
    let repo = TestRepo::new_with_daemon_scope(DaemonTestScope::NoDaemon);
    for (key, variable, default) in [
        (
            "max_transcript_line_bytes",
            "GIT_AI_MAX_TRANSCRIPT_LINE_BYTES",
            8 * 1024 * 1024,
        ),
        (
            "max_transcript_batch_bytes",
            "GIT_AI_MAX_TRANSCRIPT_BATCH_BYTES",
            8 * 1024 * 1024,
        ),
        (
            "max_transcript_file_bytes",
            "GIT_AI_MAX_TRANSCRIPT_FILE_BYTES",
            64 * 1024 * 1024,
        ),
        (
            "max_metrics_flush_chunk_bytes",
            "GIT_AI_MAX_METRICS_FLUSH_CHUNK_BYTES",
            8 * 1024 * 1024,
        ),
    ] {
        assert_eq!(get_json(&repo, key), default);
        let output = repo.git_ai(&["config", "set", key, "4096"]).unwrap();
        assert!(output.contains("git-ai bg restart"));
        assert_eq!(get_json(&repo, key), 4096);
        assert_eq!(get_json_with_env(&repo, key, &[(variable, "8192")]), 8192);
        assert_eq!(get_json_with_env(&repo, key, &[(variable, "0")]), 4096);
        assert!(repo.git_ai(&["config", "set", key, "0"]).is_err());
        let output = repo.git_ai(&["config", "unset", key]).unwrap();
        assert!(output.contains("git-ai bg restart"));
        assert_eq!(get_json(&repo, key), default);
    }
}
