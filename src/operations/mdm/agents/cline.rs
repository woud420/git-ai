use crate::error::GitAiError;
use crate::operations::mdm::extension_manifests;
use crate::operations::mdm::file_ops::{generate_diff, write_atomic};
use crate::operations::mdm::hook_installer::{HookCheckResult, HookInstaller, HookInstallerParams};
use crate::operations::mdm::paths::{home_dir, normalize_windows_path_for_shell};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
#[cfg(any(target_os = "macos", test))]
use std::sync::mpsc;
#[cfg(any(target_os = "macos", test))]
use std::thread;
#[cfg(any(target_os = "macos", test))]
use std::time::Duration;

pub struct ClineInstaller;

const MANAGED_MARKER: &str = "# git-ai-managed: cline";
const PRE_HOOK_NAME: &str = "PreToolUse";
const POST_HOOK_NAME: &str = "PostToolUse";
const CLINE_PUBLISHER_ID: &str = "saoudrizwan.claude-dev";
#[cfg(target_os = "macos")]
const CLINE_DOCUMENTS_ACCESS_TIMEOUT: Duration = Duration::from_secs(30);

impl ClineInstaller {
    fn storage_paths() -> Vec<PathBuf> {
        if let Ok(test_path) = std::env::var("GIT_AI_CLINE_STORAGE_PATH") {
            return vec![PathBuf::from(test_path)];
        }

        #[cfg(target_os = "macos")]
        let base = Some(home_dir().join("Library").join("Application Support"));

        #[cfg(target_os = "linux")]
        let base = Some(home_dir().join(".config"));

        #[cfg(target_os = "windows")]
        let base = Some(match std::env::var("APPDATA") {
            Ok(app_data) => PathBuf::from(app_data),
            Err(_) => home_dir().join("AppData").join("Roaming"),
        });

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        let base: Option<PathBuf> = None;

        let Some(base) = base else {
            return vec![];
        };

        ["Code", "Code - Insiders", "Cursor"]
            .iter()
            .map(|p| {
                base.join(p)
                    .join("User")
                    .join("globalStorage")
                    .join(CLINE_PUBLISHER_ID)
            })
            .collect()
    }

    fn hooks_dir() -> PathBuf {
        home_dir().join("Documents").join("Cline").join("Hooks")
    }

    fn hook_path(name: &str) -> PathBuf {
        Self::hooks_dir().join(name)
    }

    fn generate_hook_script(binary_path: &Path) -> String {
        let binary = normalize_windows_path_for_shell(binary_path);
        format!(
            "#!/bin/sh\n{}\n\"{}\" checkpoint cline --hook-input stdin\necho '{{\"cancel\":false}}'\n",
            MANAGED_MARKER, binary
        )
    }

    fn is_managed_script(content: &str) -> bool {
        content
            .lines()
            .any(|line| line.trim() == MANAGED_MARKER.trim())
    }

    fn read_hook_script(path: &Path) -> Result<Option<String>, GitAiError> {
        match fs::read_to_string(path) {
            Ok(content) => Ok(Some(content)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(GitAiError::Generic(format!(
                "Unable to access Cline hooks at {}: {}",
                path.display(),
                error
            ))),
        }
    }

    fn inspect_hook_scripts(binary_path: &Path) -> Result<(bool, bool), GitAiError> {
        let pre = Self::read_hook_script(&Self::hook_path(PRE_HOOK_NAME))?;
        let post = Self::read_hook_script(&Self::hook_path(POST_HOOK_NAME))?;
        let pre_managed = pre.as_deref().map(Self::is_managed_script).unwrap_or(false);
        let post_managed = post
            .as_deref()
            .map(Self::is_managed_script)
            .unwrap_or(false);
        let hooks_installed = pre_managed || post_managed;

        let expected = Self::generate_hook_script(binary_path);
        let hooks_up_to_date = pre_managed
            && post_managed
            && pre
                .as_deref()
                .is_some_and(|content| content.trim() == expected.trim())
            && post
                .as_deref()
                .is_some_and(|content| content.trim() == expected.trim());

        Ok((hooks_installed, hooks_up_to_date))
    }

    #[cfg(target_os = "macos")]
    fn preflight_documents_access() -> Result<(), GitAiError> {
        let documents = home_dir().join("Documents");
        fs::read_dir(&documents).map(drop).map_err(|error| {
            GitAiError::Generic(format!(
                "Unable to access Cline hooks in {}: {}",
                documents.display(),
                error
            ))
        })
    }

    #[cfg(any(target_os = "macos", test))]
    fn run_hook_check_with_timeout<T, F>(timeout: Duration, check: F) -> Result<T, GitAiError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, GitAiError> + Send + 'static,
    {
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        let _ = thread::spawn(move || {
            let _ = result_tx.send(check());
        });

        match result_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(GitAiError::Generic(format!(
                "Timed out checking Cline hooks after {} seconds; Cline hooks were not changed",
                timeout.as_secs()
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(GitAiError::Generic(
                "Cline hook check stopped unexpectedly".to_string(),
            )),
        }
    }

    fn is_windows() -> bool {
        cfg!(target_os = "windows")
    }

    fn ensure_hook_script_is_writable(path: &Path) -> Result<(), GitAiError> {
        if let Some(content) = Self::read_hook_script(path)?
            && !Self::is_managed_script(&content)
        {
            return Err(GitAiError::Generic(format!(
                "Refusing to overwrite unmanaged Cline hook: {}",
                path.display()
            )));
        }

        Ok(())
    }

    fn install_hook_script(
        path: &Path,
        content: &str,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        let existing = Self::read_hook_script(path)?.unwrap_or_default();

        if existing.trim() == content.trim() {
            return Ok(None);
        }

        let diff = generate_diff(path, &existing, content);

        if !dry_run {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir)?;
            }
            write_atomic(path, content.as_bytes())?;

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
            }
        }

        Ok(Some(diff))
    }

    fn uninstall_hook_script(path: &Path, dry_run: bool) -> Result<Option<String>, GitAiError> {
        let Some(existing) = Self::read_hook_script(path)? else {
            return Ok(None);
        };
        if !Self::is_managed_script(&existing) {
            return Ok(None);
        }

        let diff = generate_diff(path, &existing, "");

        if !dry_run {
            fs::remove_file(path)?;
        }

        Ok(Some(diff))
    }
}

impl HookInstaller for ClineInstaller {
    fn name(&self) -> &str {
        "Cline"
    }

    fn id(&self) -> &str {
        "cline"
    }

    fn process_names(&self) -> Vec<&str> {
        vec![]
    }

    fn uses_config_hooks(&self) -> bool {
        true
    }

    fn check_hooks(&self, params: &HookInstallerParams) -> Result<HookCheckResult, GitAiError> {
        let tool_installed = Self::storage_paths().iter().any(|p| p.exists())
            || extension_manifests::any_manifest_lists_extension(CLINE_PUBLISHER_ID);

        if !tool_installed || Self::is_windows() {
            // Cline hooks are not supported on Windows today; report the tool if it
            // is installed but leave hooks uninstalled.
            return Ok(HookCheckResult {
                tool_installed,
                hooks_installed: false,
                hooks_up_to_date: false,
            });
        }

        #[cfg(target_os = "macos")]
        let (hooks_installed, hooks_up_to_date) = {
            let binary_path = params.binary_path.clone();
            Self::run_hook_check_with_timeout(CLINE_DOCUMENTS_ACCESS_TIMEOUT, move || {
                Self::preflight_documents_access()?;
                Self::inspect_hook_scripts(&binary_path)
            })?
        };

        #[cfg(not(target_os = "macos"))]
        let (hooks_installed, hooks_up_to_date) = Self::inspect_hook_scripts(&params.binary_path)?;

        Ok(HookCheckResult {
            tool_installed,
            hooks_installed,
            hooks_up_to_date,
        })
    }

    fn install_hooks(
        &self,
        params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        if Self::is_windows() {
            return Ok(None);
        }

        let pre_path = Self::hook_path(PRE_HOOK_NAME);
        let post_path = Self::hook_path(POST_HOOK_NAME);
        Self::ensure_hook_script_is_writable(&pre_path)?;
        Self::ensure_hook_script_is_writable(&post_path)?;

        if !dry_run {
            fs::create_dir_all(Self::hooks_dir())?;
        }

        let script = Self::generate_hook_script(&params.binary_path);

        let pre_diff = Self::install_hook_script(&pre_path, &script, dry_run)?;
        let post_diff = Self::install_hook_script(&post_path, &script, dry_run)?;

        match (pre_diff, post_diff) {
            (None, None) => Ok(None),
            (Some(a), None) => Ok(Some(a)),
            (None, Some(b)) => Ok(Some(b)),
            (Some(a), Some(b)) => Ok(Some(format!("{}\n{}", a, b))),
        }
    }

    fn uninstall_hooks(
        &self,
        _params: &HookInstallerParams,
        dry_run: bool,
    ) -> Result<Option<String>, GitAiError> {
        if Self::is_windows() {
            return Ok(None);
        }

        let pre_path = Self::hook_path(PRE_HOOK_NAME);
        let post_path = Self::hook_path(POST_HOOK_NAME);

        let pre_diff = Self::uninstall_hook_script(&pre_path, dry_run)?;
        let post_diff = Self::uninstall_hook_script(&post_path, dry_run)?;

        match (pre_diff, post_diff) {
            (None, None) => Ok(None),
            (Some(a), None) => Ok(Some(a)),
            (None, Some(b)) => Ok(Some(b)),
            (Some(a), Some(b)) => Ok(Some(format!("{}\n{}", a, b))),
        }
    }
}

#[cfg(test)]
mod tests;
