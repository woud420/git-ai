//! Canonical metadata for the `git-ai config` command.
//!
//! The command keeps typed mutation code in the get/set/unset modules, but all
//! public key names, aliases, nesting rules, and help text live here.  This
//! makes adding a persisted setting an explicit, reviewable change instead of
//! another independent string match in each command.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfigValueKind {
    String,
    Boolean,
    Integer,
    Array,
    Object,
    Secret,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfigNesting {
    None,
    /// The parent has a fixed set of known child paths, but the command owns
    /// the child-specific error text and validation.
    Fixed,
    /// The parent accepts user-defined child names (for example feature flags
    /// and custom attributes).
    Dynamic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ConfigMutation {
    Replace,
    ReplaceOrAdd,
    NestedOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ConfigKeySpec {
    pub(super) name: &'static str,
    pub(super) aliases: &'static [&'static str],
    pub(super) value_kind: ConfigValueKind,
    pub(super) sensitive: bool,
    pub(super) nesting: ConfigNesting,
    pub(super) mutation: ConfigMutation,
    /// The complete line printed by `git-ai config --help`, without the
    /// command's two-space indentation prefix.
    pub(super) help: &'static str,
    pub(super) show_in_help: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ResolvedConfigKey {
    pub(super) spec: &'static ConfigKeySpec,
}

impl ResolvedConfigKey {
    /// Return the top-level path component used by the typed handlers.
    pub(super) fn root(self) -> &'static str {
        self.spec.name.split('.').next().unwrap_or(self.spec.name)
    }
}

const NO_ALIASES: &[&str] = &[];

impl ConfigKeySpec {
    /// A visible, non-nested key. Collections (arrays and objects) accept
    /// `--add`; secrets are always sensitive.
    const fn new(name: &'static str, value_kind: ConfigValueKind, help: &'static str) -> Self {
        let mutation = match value_kind {
            ConfigValueKind::Array | ConfigValueKind::Object => ConfigMutation::ReplaceOrAdd,
            _ => ConfigMutation::Replace,
        };
        Self {
            name,
            aliases: NO_ALIASES,
            value_kind,
            sensitive: matches!(value_kind, ConfigValueKind::Secret),
            nesting: ConfigNesting::None,
            mutation,
            help,
            show_in_help: true,
        }
    }

    /// An object parent that only exists so nested paths resolve; it is not
    /// listed in help output.
    const fn hidden_parent(
        name: &'static str,
        nesting: ConfigNesting,
        mutation: ConfigMutation,
    ) -> Self {
        Self {
            nesting,
            mutation,
            show_in_help: false,
            ..Self::new(name, ConfigValueKind::Object, "")
        }
    }

    const fn aliases(self, aliases: &'static [&'static str]) -> Self {
        Self { aliases, ..self }
    }

    const fn nesting(self, nesting: ConfigNesting) -> Self {
        Self { nesting, ..self }
    }
}

// Keep this order aligned with the existing help output.  Hidden parent
// entries are retained in the registry so nested paths resolve through the
// same metadata as top-level paths without changing that output.
const CONFIG_KEY_SPECS: &[ConfigKeySpec] = &[
    ConfigKeySpec::new(
        "git_path",
        ConfigValueKind::String,
        "git_path                     Path to git binary",
    ),
    ConfigKeySpec::new(
        "exclude_prompts_in_repositories",
        ConfigValueKind::Array,
        "exclude_prompts_in_repositories  Repos to exclude prompts from (array)",
    ),
    ConfigKeySpec::new(
        "allowed_repositories",
        ConfigValueKind::Array,
        "allowed_repositories         Repositories where collection is enabled (array; empty = collect nothing)",
    )
    .aliases(&["allow_repositories"]),
    ConfigKeySpec::new(
        "exclude_repositories",
        ConfigValueKind::Array,
        "exclude_repositories         Excluded repos (array)",
    ),
    ConfigKeySpec::new(
        "telemetry",
        ConfigValueKind::String,
        "telemetry                    Master telemetry switch (on/off; default off)",
    ),
    ConfigKeySpec::new(
        "telemetry_oss",
        ConfigValueKind::String,
        "telemetry_oss                Legacy OSS telemetry setting (on/off)",
    )
    .aliases(&["telemetry_oss_disabled"]),
    ConfigKeySpec::new(
        "telemetry_enterprise_dsn",
        ConfigValueKind::String,
        "telemetry_enterprise_dsn     Enterprise telemetry DSN",
    ),
    ConfigKeySpec::new(
        "disable_version_checks",
        ConfigValueKind::Boolean,
        "disable_version_checks       Disable version checks (bool)",
    ),
    ConfigKeySpec::new(
        "disable_auto_updates",
        ConfigValueKind::Boolean,
        "disable_auto_updates         Disable auto updates (bool)",
    ),
    ConfigKeySpec::new(
        "update_channel",
        ConfigValueKind::String,
        "update_channel               Update channel (latest/next/enterprise-latest/enterprise-next)",
    ),
    ConfigKeySpec::new(
        "feature_flags",
        ConfigValueKind::Object,
        "feature_flags                Feature flags (object)",
    )
    .nesting(ConfigNesting::Dynamic),
    ConfigKeySpec::new(
        "feature_flags.whitelist_agent_sandboxes",
        ConfigValueKind::Boolean,
        "feature_flags.whitelist_agent_sandboxes  Allow the active trace socket in supported agent sandboxes on install-hooks (bool; default false)",
    ),
    ConfigKeySpec::new(
        "api_base_url",
        ConfigValueKind::String,
        "api_base_url                 API base URL (default: https://usegitai.com)",
    ),
    ConfigKeySpec::new(
        "api_key",
        ConfigValueKind::Secret,
        "api_key                      API key for X-API-Key header",
    ),
    ConfigKeySpec::hidden_parent("author", ConfigNesting::Fixed, ConfigMutation::ReplaceOrAdd),
    ConfigKeySpec::new(
        "author.name",
        ConfigValueKind::String,
        "author.name                  git-ai author display name override",
    ),
    ConfigKeySpec::new(
        "author.email",
        ConfigValueKind::String,
        "author.email                 git-ai author email override",
    ),
    ConfigKeySpec::new(
        "prompt_storage",
        ConfigValueKind::String,
        "prompt_storage               Prompt storage mode (default/notes/local)",
    ),
    ConfigKeySpec::new(
        "include_prompts_in_repositories",
        ConfigValueKind::Array,
        "include_prompts_in_repositories  Repos to include for prompt storage (array)",
    ),
    ConfigKeySpec::new(
        "default_prompt_storage",
        ConfigValueKind::String,
        "default_prompt_storage       Fallback storage mode for non-included repos",
    ),
    ConfigKeySpec::new(
        "quiet",
        ConfigValueKind::Boolean,
        "quiet                        Suppress chart output after commits (bool)",
    ),
    ConfigKeySpec::new(
        "allow_superuser",
        ConfigValueKind::Boolean,
        "allow_superuser              Allow running git-ai as root/superuser (bool)",
    ),
    ConfigKeySpec::new(
        "transcript_streaming_lookback_days",
        ConfigValueKind::Integer,
        "transcript_streaming_lookback_days  Days to look back when sweeping transcripts (0 = unlimited)",
    ),
    ConfigKeySpec::new(
        "max_checkpoint_file_size_bytes",
        ConfigValueKind::Integer,
        "max_checkpoint_file_size_bytes      Per-file checkpoint content limit in bytes",
    ),
    ConfigKeySpec::new(
        "max_checkpoint_total_size_bytes",
        ConfigValueKind::Integer,
        "max_checkpoint_total_size_bytes     Per-checkpoint content limit in bytes",
    ),
    ConfigKeySpec::new(
        "max_checkpoint_total_lines",
        ConfigValueKind::Integer,
        "max_checkpoint_total_lines          Per-checkpoint content limit in lines",
    ),
    ConfigKeySpec::new(
        "max_transcript_line_bytes",
        ConfigValueKind::Integer,
        "max_transcript_line_bytes         Transcript line or SQLite event limit in bytes",
    ),
    ConfigKeySpec::new(
        "max_transcript_batch_bytes",
        ConfigValueKind::Integer,
        "max_transcript_batch_bytes         JSONL or SQLite transcript batch limit in bytes",
    ),
    ConfigKeySpec::new(
        "max_transcript_file_bytes",
        ConfigValueKind::Integer,
        "max_transcript_file_bytes         Whole-file JSON transcript limit in bytes",
    ),
    ConfigKeySpec::new(
        "max_metrics_flush_chunk_bytes",
        ConfigValueKind::Integer,
        "max_metrics_flush_chunk_bytes         Queued metrics payload limit per flush batch in bytes",
    ),
    ConfigKeySpec::new(
        "daemon_memory_limit_mb",
        ConfigValueKind::Integer,
        "daemon_memory_limit_mb              Daemon current-RSS limit in MiB",
    ),
    ConfigKeySpec::new(
        "custom_attributes",
        ConfigValueKind::Object,
        "custom_attributes            Custom telemetry attributes, string->string (object)",
    )
    .nesting(ConfigNesting::Dynamic),
    ConfigKeySpec::new(
        "git_ai_hooks",
        ConfigValueKind::Object,
        "git_ai_hooks                 Hook name -> shell commands map (object)",
    )
    .nesting(ConfigNesting::Dynamic),
    ConfigKeySpec::new(
        "codex_hooks_format",
        ConfigValueKind::String,
        "codex_hooks_format           Codex hook install format (config_toml/hooks_json)",
    ),
    ConfigKeySpec::hidden_parent("notes_backend", ConfigNesting::Fixed, ConfigMutation::NestedOnly),
    ConfigKeySpec::new(
        "notes_backend.kind",
        ConfigValueKind::String,
        "notes_backend.kind           Notes backend kind (sqlite/git_notes/http)",
    ),
    ConfigKeySpec::new(
        "notes_backend.backend_url",
        ConfigValueKind::String,
        "notes_backend.backend_url    Notes backend base URL. Required when kind=http.",
    ),
];

pub(super) fn config_key_specs() -> &'static [ConfigKeySpec] {
    CONFIG_KEY_SPECS
}

pub(super) fn resolve_key(key: &str) -> Result<ResolvedConfigKey, String> {
    let key = key.trim();

    if let Some(spec) = CONFIG_KEY_SPECS
        .iter()
        .find(|spec| spec.name == key || spec.aliases.contains(&key))
    {
        return Ok(ResolvedConfigKey { spec });
    }

    let root = key.split('.').next().unwrap_or_default();
    if let Some(spec) = CONFIG_KEY_SPECS
        .iter()
        .find(|spec| spec.name == root && spec.nesting != ConfigNesting::None)
    {
        return Ok(ResolvedConfigKey { spec });
    }

    Err(format!("Unknown config key: {}", key))
}
