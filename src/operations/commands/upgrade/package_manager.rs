use super::DaemonUpdateCheckResult;
use crate::operations::mdm::paths::get_current_binary_path;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PackageManager {
    Homebrew,
    Chocolatey,
}

impl PackageManager {
    pub(super) fn upgrade_instruction(self) -> &'static str {
        match self {
            Self::Homebrew => {
                "This installation is managed by Homebrew. Use `brew upgrade --fetch-HEAD woud420/git-ai/git-ai`, then run `git-ai install-hooks`."
            }
            Self::Chocolatey => {
                "This installation is managed by Chocolatey. Use `choco upgrade git-ai --source <package-directory>`, then run `git-ai install-hooks` as your normal user."
            }
        }
    }
}

pub(super) fn current() -> Option<PackageManager> {
    // Package ownership belongs to the resolved executable, not per-user config.
    // Cache the marker so recurring async update checks do not reread it.
    static MANAGER: OnceLock<Option<PackageManager>> = OnceLock::new();
    *MANAGER.get_or_init(|| {
        get_current_binary_path()
            .ok()
            .and_then(|binary| for_binary(&binary))
    })
}

fn for_binary(binary: &Path) -> Option<PackageManager> {
    let marker = binary.parent()?.join("git-ai-package-manager");
    match std::fs::read_to_string(marker).ok()?.trim() {
        "homebrew" => Some(PackageManager::Homebrew),
        "chocolatey" => Some(PackageManager::Chocolatey),
        _ => None,
    }
}

pub(super) fn automatic_update(
    manager: Option<PackageManager>,
    update: impl FnOnce() -> Result<DaemonUpdateCheckResult, String>,
) -> Result<DaemonUpdateCheckResult, String> {
    if manager.is_some() {
        Ok(DaemonUpdateCheckResult::NoUpdate)
    } else {
        update()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_manager_marker_is_adjacent_and_recognized() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("git-ai");
        let marker = directory.path().join("git-ai-package-manager");
        assert_eq!(for_binary(&binary), None);
        for (contents, expected) in [
            ("homebrew\n", Some(PackageManager::Homebrew)),
            ("chocolatey\r\n", Some(PackageManager::Chocolatey)),
            ("unknown", None),
            ("", None),
        ] {
            std::fs::write(&marker, contents).unwrap();
            assert_eq!(for_binary(&binary), expected);
        }
        std::fs::write(&marker, "homebrew").unwrap();
        let other_directory = tempfile::tempdir().unwrap();
        assert_eq!(for_binary(&other_directory.path().join("git-ai")), None);
    }

    #[test]
    fn package_manager_automatic_update_skips_release_checks_and_pending_installers() {
        for manager in [PackageManager::Homebrew, PackageManager::Chocolatey] {
            let result = automatic_update(Some(manager), || {
                panic!("package-owned binaries must not check releases or run installers")
            });
            assert_eq!(result.unwrap(), DaemonUpdateCheckResult::NoUpdate);
        }
    }

    #[test]
    fn package_manager_automatic_update_preserves_unmanaged_results_and_errors() {
        assert_eq!(
            automatic_update(None, || Ok(DaemonUpdateCheckResult::UpdateReady)).unwrap(),
            DaemonUpdateCheckResult::UpdateReady
        );
        assert_eq!(
            automatic_update(None, || Err("release unavailable".into())).unwrap_err(),
            "release unavailable"
        );
    }
}
