use std::collections::{HashMap, hash_map::DefaultHasher};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use glob::Pattern;
use serde::Serialize;

use crate::feature_flags::FeatureFlags;
use crate::operations::mdm::paths::home_dir;

mod author;
mod budgets;
mod file;
mod notes_backend;
mod patterns;
#[cfg(any(test, feature = "test-support"))]
mod test_support;

#[cfg(test)]
pub(crate) mod tests;

// --- Public re-exports (preserve every crate::config::X path) ---

pub use author::AuthorConfig;
pub use budgets::{
    DEFAULT_MAX_METRICS_FLUSH_CHUNK_BYTES, DEFAULT_MAX_TRANSCRIPT_BATCH_BYTES,
    DEFAULT_MAX_TRANSCRIPT_FILE_BYTES, DEFAULT_MAX_TRANSCRIPT_LINE_BYTES,
};
#[cfg(any(test, feature = "test-support"))]
pub use file::ConfigPatch;
pub use file::{
    CodexHooksFormat, FileConfig, UpdateChannel, config_file_path, is_real_git_candidate,
    load_file_config_public, save_file_config,
};
pub use notes_backend::{NotesBackendConfig, NotesBackendKind};

// Internal re-exports used by submodules or tests
pub(crate) use file::build_config;
pub(crate) use file::strip_utf8_bom;
pub(crate) use patterns::{remote_matches_patterns, repo_root_matches_patterns};

/// Default API base URL for comparison
pub const DEFAULT_API_BASE_URL: &str = "https://usegitai.com";
pub const DEFAULT_MAX_CHECKPOINT_FILE_SIZE_BYTES: usize = 3 * 1024 * 1024;
pub const DEFAULT_MAX_CHECKPOINT_TOTAL_SIZE_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_MAX_CHECKPOINT_TOTAL_LINES: usize = 500_000;
pub(crate) const MEBIBYTE_BYTES: u64 = 1024 * 1024;
pub(crate) const MAX_DAEMON_MEMORY_LIMIT_MB: u64 = u64::MAX / MEBIBYTE_BYTES;

#[derive(Serialize)]
pub struct Config {
    pub(crate) git_path: String,
    #[serde(serialize_with = "patterns::serialize_patterns")]
    pub(crate) exclude_prompts_in_repositories: Vec<Pattern>,
    #[serde(serialize_with = "patterns::serialize_patterns")]
    pub(crate) include_prompts_in_repositories: Vec<Pattern>,
    #[serde(serialize_with = "patterns::serialize_patterns")]
    pub(crate) allowed_repositories: Vec<Pattern>,
    #[serde(serialize_with = "patterns::serialize_patterns")]
    pub(crate) exclude_repositories: Vec<Pattern>,
    pub(crate) telemetry_enabled: bool,
    pub(crate) telemetry_oss_disabled: bool,
    pub(crate) telemetry_enterprise_dsn: Option<String>,
    pub(crate) disable_version_checks: bool,
    pub(crate) disable_auto_updates: bool,
    pub(crate) update_channel: UpdateChannel,
    pub(crate) feature_flags: FeatureFlags,
    pub(crate) api_base_url: String,
    pub(crate) prompt_storage: String,
    pub(crate) default_prompt_storage: Option<String>,
    #[serde(serialize_with = "serialize_masked_api_key")]
    pub(crate) api_key: Option<String>,
    pub(crate) quiet: bool,
    pub(crate) allow_superuser: bool,
    pub(crate) author: AuthorConfig,
    pub(crate) custom_attributes: HashMap<String, String>,
    pub(crate) git_ai_hooks: HashMap<String, Vec<String>>,
    pub(crate) codex_hooks_format: CodexHooksFormat,
    pub(crate) notes_backend: NotesBackendConfig,
    pub(crate) transcript_streaming_lookback_days: Option<u32>,
    pub(crate) max_checkpoint_file_size_bytes: usize,
    pub(crate) max_checkpoint_total_size_bytes: usize,
    pub(crate) max_checkpoint_total_lines: usize,
    pub(crate) max_transcript_line_bytes: usize,
    pub(crate) max_transcript_batch_bytes: usize,
    pub(crate) max_transcript_file_bytes: usize,
    pub(crate) max_metrics_flush_chunk_bytes: usize,
    pub(crate) daemon_memory_limit_mb: Option<u64>,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

const AUTHOR_CONFIG_CACHE_TTL: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuthorConfigCacheKey {
    config_path: Option<PathBuf>,
    config_fingerprint: Option<AuthorConfigFileFingerprint>,
    #[cfg(any(test, feature = "test-support"))]
    test_patch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthorConfigFileFingerprint {
    pub(crate) len: u64,
    pub(crate) hash: u64,
}

#[derive(Debug, Clone)]
struct CachedAuthorConfig {
    key: AuthorConfigCacheKey,
    loaded_at: Instant,
    author: AuthorConfig,
}

static AUTHOR_CONFIG_CACHE: OnceLock<Mutex<Option<CachedAuthorConfig>>> = OnceLock::new();

impl Config {
    /// Access the global configuration. Lazily initializes if not already initialized.
    pub fn get() -> &'static Config {
        CONFIG.get_or_init(build_config)
    }

    /// Build a fresh config snapshot from disk/env without using the global cache.
    ///
    /// This is useful for long-lived daemon processes that must observe runtime
    /// config updates (for example, prompt sharing/privacy toggles).
    pub fn fresh() -> Self {
        build_config()
    }

    /// Return the fresh author override with a short process-local TTL.
    ///
    /// Author identity is consulted in hot paths such as checkpoint bursts and
    /// daemon replay. This avoids the global `Config::get()` singleton while
    /// still bounding repeated config file reads during a burst of operations.
    pub fn fresh_author_cached() -> AuthorConfig {
        let key = author_config_cache_key();
        let now = Instant::now();
        let cache = AUTHOR_CONFIG_CACHE.get_or_init(|| Mutex::new(None));
        if let Ok(mut guard) = cache.lock() {
            if let Some(cached) = guard.as_ref()
                && cached.key == key
                && now.duration_since(cached.loaded_at) < AUTHOR_CONFIG_CACHE_TTL
            {
                return cached.author.clone();
            }

            let author = build_config().author;
            *guard = Some(CachedAuthorConfig {
                key,
                loaded_at: now,
                author: author.clone(),
            });
            return author;
        }

        build_config().author
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_author_config_cache_for_tests() {
        if let Some(cache) = AUTHOR_CONFIG_CACHE.get()
            && let Ok(mut guard) = cache.lock()
        {
            *guard = None;
        }
    }

    /// Returns the command to invoke git.
    pub fn git_cmd(&self) -> &str {
        &self.git_path
    }

    /// True when at least one repository is allowed for collection.
    /// Collection is opt-in: with an empty allowlist git-ai collects nothing.
    pub fn has_allowed_repositories(&self) -> bool {
        !self.allowed_repositories.is_empty()
    }

    /// Helper that accepts pre-fetched remotes and the repository root to avoid
    /// repeated git operations. Collection is opt-in: an empty
    /// `allowed_repositories` list denies every repository. Entries match
    /// either a remote URL or the repository root path (glob patterns; a plain
    /// directory entry also matches every repository beneath it).
    #[cfg_attr(test, allow(dead_code))]
    pub(crate) fn is_allowed_repository_with_context(
        &self,
        remotes: Option<&Vec<(String, String)>>,
        repo_root: Option<&Path>,
    ) -> bool {
        if remotes.is_some_and(|remotes| {
            remotes.iter().any(|(_, remote_url)| {
                crate::diagnostic_sentinels::is_debug_self_check_remote_url(remote_url)
            })
        }) {
            return true;
        }

        // Exclusions take precedence over the allowlist.
        if !self.exclude_repositories.is_empty() {
            if let Some(remotes) = remotes
                && remotes
                    .iter()
                    .any(|remote| remote_matches_patterns(&self.exclude_repositories, &remote.1))
            {
                return false;
            }
            if let Some(root) = repo_root
                && repo_root_matches_patterns(&self.exclude_repositories, root)
            {
                return false;
            }
        }

        // Collection is opt-in: an empty allowlist denies everything.
        if self.allowed_repositories.is_empty() {
            return false;
        }

        let remote_allowed = remotes.is_some_and(|remotes| {
            remotes
                .iter()
                .any(|remote| remote_matches_patterns(&self.allowed_repositories, &remote.1))
        });

        remote_allowed
            || repo_root
                .is_some_and(|root| repo_root_matches_patterns(&self.allowed_repositories, root))
    }

    /// Returns true if OSS telemetry is disabled.
    /// Master telemetry switch. Telemetry is off by default; egress
    /// (Sentry/PostHog, metrics upload, daemon log upload, heartbeats) only
    /// runs when the `telemetry` config key is "on" (or the legacy
    /// `telemetry_oss` key is "on"). An explicitly configured
    /// `telemetry_enterprise_dsn` is its own opt-in and is not gated here.
    pub fn telemetry_enabled(&self) -> bool {
        self.telemetry_enabled
    }

    pub fn is_telemetry_oss_disabled(&self) -> bool {
        self.telemetry_oss_disabled
    }

    /// Returns the telemetry_enterprise_dsn if set.
    pub fn telemetry_enterprise_dsn(&self) -> Option<&str> {
        self.telemetry_enterprise_dsn.as_deref()
    }

    pub fn version_checks_disabled(&self) -> bool {
        self.disable_version_checks
    }

    pub fn auto_updates_disabled(&self) -> bool {
        self.disable_auto_updates
    }

    pub fn update_channel(&self) -> UpdateChannel {
        self.update_channel
    }

    pub fn feature_flags(&self) -> &FeatureFlags {
        &self.feature_flags
    }

    /// Returns the API base URL
    pub fn api_base_url(&self) -> &str {
        &self.api_base_url
    }

    /// Returns the prompt storage mode: "default", "notes", or "local"
    /// - "default": Messages uploaded via CAS API
    /// - "notes": Messages stored in git notes
    /// - "local": Messages only stored in sqlite (not in notes, not uploaded)
    pub fn prompt_storage(&self) -> &str {
        &self.prompt_storage
    }

    /// Returns the API key if configured
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// Returns the notes backend config.
    pub fn notes_backend(&self) -> &NotesBackendConfig {
        &self.notes_backend
    }

    /// Returns the notes backend kind.
    pub fn notes_backend_kind(&self) -> NotesBackendKind {
        self.notes_backend.kind
    }

    /// Returns the configured notes backend URL, or `None` if unset.
    ///
    /// Callers must handle `None` explicitly — typically by skipping the operation when the HTTP backend
    /// is enabled but no URL has been configured.
    pub fn notes_backend_url(&self) -> Option<&str> {
        self.notes_backend.backend_url.as_deref()
    }

    /// Returns true when the HTTP notes backend is active.
    pub fn notes_backend_enabled(&self) -> bool {
        matches!(self.notes_backend.kind, NotesBackendKind::Http)
    }

    pub fn transcript_streaming_lookback_days(&self) -> Option<u32> {
        self.transcript_streaming_lookback_days
    }

    /// Returns the daemon current-RSS limit in MiB, or `None` when disabled.
    pub fn daemon_memory_limit_mb(&self) -> Option<u64> {
        self.daemon_memory_limit_mb
    }

    /// Returns true if quiet mode is enabled (suppresses chart output after commits)
    pub fn is_quiet(&self) -> bool {
        self.quiet
    }

    pub fn allow_superuser(&self) -> bool {
        self.allow_superuser
    }

    /// Returns the configured git-ai author override.
    pub fn author(&self) -> &AuthorConfig {
        &self.author
    }

    /// Returns the custom attributes map (from config file + env var override).
    pub fn custom_attributes(&self) -> &HashMap<String, String> {
        &self.custom_attributes
    }

    /// Returns all configured git-ai hook commands.
    pub fn git_ai_hooks(&self) -> &HashMap<String, Vec<String>> {
        &self.git_ai_hooks
    }

    /// Returns configured shell commands for a specific hook.
    pub fn git_ai_hook_commands(&self, hook_name: &str) -> Option<&Vec<String>> {
        self.git_ai_hooks.get(hook_name)
    }

    pub fn codex_hooks_format(&self) -> CodexHooksFormat {
        self.codex_hooks_format
    }

    /// Serialize the effective runtime config into pretty JSON.
    /// Sensitive values are redacted via field serializers.
    pub fn to_printable_json_pretty(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize runtime config: {}", e))
    }

    /// Override feature flags for testing purposes.
    /// Only available when the `test-support` feature is enabled or in test mode.
    /// Must be `pub` to work with integration tests in the `tests/` directory.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub fn set_test_feature_flags(flags: FeatureFlags) {
        let mut override_flags = file::TEST_FEATURE_FLAGS_OVERRIDE
            .write()
            .expect("Failed to acquire write lock on test feature flags");
        *override_flags = Some(flags);
    }

    /// Clear any feature flag overrides.
    /// Only available when the `test-support` feature is enabled or in test mode.
    /// This should be called in test cleanup to reset to default behavior.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)]
    pub fn clear_test_feature_flags() {
        let mut override_flags = file::TEST_FEATURE_FLAGS_OVERRIDE
            .write()
            .expect("Failed to acquire write lock on test feature flags");
        *override_flags = None;
    }

    /// Get feature flags, checking for test overrides first.
    /// In test mode, this will return overridden flags if set, otherwise the normal flags.
    #[cfg(any(test, feature = "test-support"))]
    pub fn get_feature_flags(&self) -> FeatureFlags {
        let override_flags = file::TEST_FEATURE_FLAGS_OVERRIDE
            .read()
            .expect("Failed to acquire read lock on test feature flags");
        override_flags
            .clone()
            .unwrap_or_else(|| self.feature_flags.clone())
    }

    /// Get feature flags (non-test version, just returns a reference).
    #[cfg(not(any(test, feature = "test-support")))]
    pub fn get_feature_flags(&self) -> &FeatureFlags {
        &self.feature_flags
    }
}

/// Redact the entire credential, including its length, on every display path.
pub(crate) fn mask_api_key(_key: &str) -> String {
    "****".to_string()
}

fn serialize_masked_api_key<S>(api_key: &Option<String>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    use serde::Serialize;
    api_key.as_deref().map(mask_api_key).serialize(serializer)
}

fn author_config_cache_key() -> AuthorConfigCacheKey {
    let config_path = file::config_file_path();
    let config_fingerprint = config_path
        .as_ref()
        .and_then(|path| author_config_file_fingerprint(path));

    AuthorConfigCacheKey {
        config_path,
        config_fingerprint,
        #[cfg(any(test, feature = "test-support"))]
        test_patch: std::env::var("GIT_AI_TEST_CONFIG_PATCH").ok(),
    }
}

pub(crate) fn author_config_file_fingerprint(path: &Path) -> Option<AuthorConfigFileFingerprint> {
    let data = fs::read(path).ok()?;
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    Some(AuthorConfigFileFingerprint {
        len: data.len() as u64,
        hash: hasher.finish(),
    })
}

/// Returns the path to the git-ai base directory (~/.git-ai)
pub fn git_ai_dir_path() -> Option<PathBuf> {
    Some(home_dir().join(".git-ai"))
}

/// Returns the path to the internal state directory (~/.git-ai/internal)
/// This is where git-ai stores internal files like distinct_id, update_check, etc.
pub fn internal_dir_path() -> Option<PathBuf> {
    git_ai_dir_path().map(|dir| dir.join("internal"))
}

/// Returns the path to the skills directory (~/.git-ai/skills)
/// This is where git-ai installs skills for Claude Code and other agents
pub fn skills_dir_path() -> Option<PathBuf> {
    git_ai_dir_path().map(|dir| dir.join("skills"))
}

/// Public accessor for ID file path (~/.git-ai/internal/distinct_id)
pub fn id_file_path() -> Option<PathBuf> {
    internal_dir_path().map(|dir| dir.join("distinct_id"))
}

/// Cache for the distinct_id to avoid repeated file reads
static DISTINCT_ID: OnceLock<String> = OnceLock::new();

/// Get or create the distinct_id (UUID) from ~/.git-ai/internal/distinct_id
/// If the file doesn't exist, generates a new UUID and writes it to the file.
/// The result is cached for the lifetime of the process.
pub fn get_or_create_distinct_id() -> String {
    DISTINCT_ID
        .get_or_init(|| {
            let id_path = match id_file_path() {
                Some(path) => path,
                None => return "unknown".to_string(),
            };

            // Try to read existing ID
            if let Ok(existing_id) = fs::read_to_string(&id_path) {
                let trimmed = existing_id.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }

            // Generate new UUID
            let new_id = crate::uuid::generate_v4();

            // Ensure directory exists
            if let Some(parent) = id_path.parent() {
                let _ = fs::create_dir_all(parent);
            }

            // Write the new ID to file
            if let Err(e) = fs::write(&id_path, &new_id) {
                eprintln!("Warning: Failed to write distinct_id file: {}", e);
            }

            new_id
        })
        .clone()
}

/// Returns the path to the update check cache file (~/.git-ai/internal/update_check)
pub fn update_check_path() -> Option<PathBuf> {
    internal_dir_path().map(|dir| dir.join("update_check"))
}
