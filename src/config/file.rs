mod assembly;
pub(crate) use assembly::build_config;
pub use assembly::is_real_git_candidate;
#[cfg(all(test, unix))]
pub(crate) use assembly::path_is_git_ai_binary;
pub(crate) use assembly::strip_utf8_bom;
pub use assembly::{config_file_path, load_file_config_public, save_file_config};
#[cfg(test)]
pub(crate) use assembly::{parse_file_config_bytes, resolve_telemetry_enabled};

const TRACING_TARGET: &str = module_path!();

use super::author::AuthorConfig;
use super::notes_backend::NotesBackendConfig;
#[cfg(any(test, feature = "test-support"))]
use crate::feature_flags::FeatureFlags;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(any(test, feature = "test-support"))]
use std::sync::RwLock;
#[cfg(any(test, feature = "test-support"))]
pub(crate) static TEST_FEATURE_FLAGS_OVERRIDE: RwLock<Option<FeatureFlags>> = RwLock::new(None);

/// Which Codex hook file git-ai should use when installing Codex hooks.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodexHooksFormat {
    /// Default: install git-ai Codex hooks inline in ~/.codex/config.toml.
    #[default]
    ConfigToml,
    /// Install git-ai Codex hooks in ~/.codex/hooks.json.
    HooksJson,
}

impl CodexHooksFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodexHooksFormat::ConfigToml => "config_toml",
            CodexHooksFormat::HooksJson => "hooks_json",
        }
    }

    pub(crate) fn from_str(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "config_toml" | "config-toml" => Some(CodexHooksFormat::ConfigToml),
            "hooks_json" | "hooks-json" => Some(CodexHooksFormat::HooksJson),
            _ => None,
        }
    }
}

impl std::fmt::Display for CodexHooksFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateChannel {
    #[default]
    Latest,
    Next,
    EnterpriseLatest,
    EnterpriseNext,
}

impl UpdateChannel {
    pub fn as_str(&self) -> &'static str {
        match self {
            UpdateChannel::Latest => "latest",
            UpdateChannel::Next => "next",
            UpdateChannel::EnterpriseLatest => "enterprise-latest",
            UpdateChannel::EnterpriseNext => "enterprise-next",
        }
    }

    pub(crate) fn from_str(input: &str) -> Option<Self> {
        match input.trim().to_lowercase().as_str() {
            "latest" => Some(UpdateChannel::Latest),
            "next" => Some(UpdateChannel::Next),
            "enterprise-latest" => Some(UpdateChannel::EnterpriseLatest),
            "enterprise-next" => Some(UpdateChannel::EnterpriseNext),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize, Default)]
pub struct FileConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_prompts_in_repositories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_prompts_in_repositories: Option<Vec<String>>,
    #[serde(
        default,
        alias = "allow_repositories",
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_repositories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_repositories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_oss: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_enterprise_dsn: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_version_checks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_auto_updates: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_storage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_prompt_storage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_superuser: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<AuthorConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_attributes: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_ai_hooks: Option<HashMap<String, Vec<String>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_hooks_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_backend: Option<NotesBackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_streaming_lookback_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_checkpoint_file_size_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_checkpoint_total_size_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_checkpoint_total_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_memory_limit_mb: Option<u64>,
}

/// Serializable config patch for test overrides
/// All fields are optional to allow patching only specific properties
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_prompts_in_repositories: Option<Vec<String>>,
    #[serde(
        default,
        alias = "allow_repositories",
        skip_serializing_if = "Option::is_none"
    )]
    pub allowed_repositories: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_oss_disabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_version_checks: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_auto_updates: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_storage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<AuthorConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_attributes: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_hooks_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_backend: Option<NotesBackendConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_streaming_lookback_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_checkpoint_file_size_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_checkpoint_total_size_bytes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_checkpoint_total_lines: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_memory_limit_mb: Option<u64>,
}
