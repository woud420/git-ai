use super::*;

pub(crate) fn create_test_config(
    allowed_repositories: Vec<String>,
    exclude_repositories: Vec<String>,
) -> Config {
    Config {
        git_path: "/usr/bin/git".to_string(),
        exclude_prompts_in_repositories: vec![],
        include_prompts_in_repositories: vec![],
        allowed_repositories: allowed_repositories
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        exclude_repositories: exclude_repositories
            .into_iter()
            .filter_map(|s| Pattern::new(&s).ok())
            .collect(),
        telemetry_enabled: false,
        telemetry_oss_disabled: false,
        telemetry_enterprise_dsn: None,
        disable_version_checks: false,
        disable_auto_updates: false,
        update_channel: UpdateChannel::Latest,
        feature_flags: FeatureFlags::default(),
        api_base_url: DEFAULT_API_BASE_URL.to_string(),
        prompt_storage: "default".to_string(),
        default_prompt_storage: None,
        api_key: None,
        quiet: false,
        allow_superuser: false,
        author: AuthorConfig::default(),
        custom_attributes: HashMap::new(),
        git_ai_hooks: HashMap::new(),
        codex_hooks_format: CodexHooksFormat::ConfigToml,
        notes_backend: NotesBackendConfig::default(),
        transcript_streaming_lookback_days: Some(7),
        max_checkpoint_file_size_bytes: DEFAULT_MAX_CHECKPOINT_FILE_SIZE_BYTES,
        max_checkpoint_total_size_bytes: DEFAULT_MAX_CHECKPOINT_TOTAL_SIZE_BYTES,
        max_checkpoint_total_lines: DEFAULT_MAX_CHECKPOINT_TOTAL_LINES,
        max_transcript_line_bytes: super::super::DEFAULT_MAX_TRANSCRIPT_LINE_BYTES,
        max_transcript_batch_bytes: super::super::DEFAULT_MAX_TRANSCRIPT_BATCH_BYTES,
        max_transcript_file_bytes: super::super::DEFAULT_MAX_TRANSCRIPT_FILE_BYTES,
        max_metrics_flush_chunk_bytes: super::super::DEFAULT_MAX_METRICS_FLUSH_CHUNK_BYTES,
        daemon_memory_limit_mb: None,
    }
}
