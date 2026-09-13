use super::metadata::{get_plugins_dir, read_product_info_build_metadata};
use crate::operations::mdm::jetbrains::ide_types::{DetectedIde, JETBRAINS_IDES, JetBrainsIde};
use std::path::{Path, PathBuf};
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
};

// ===== Windows Detection =====

#[cfg(windows)]
pub(super) fn find_windows_installations() -> Vec<DetectedIde> {
    let mut detected = Vec::new();

    // Scan Toolbox directory
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let toolbox_apps = PathBuf::from(&local_app_data)
            .join("JetBrains")
            .join("Toolbox")
            .join("apps");

        if toolbox_apps.exists() {
            detected.extend(scan_windows_toolbox_dir(&toolbox_apps));
        }
    }

    // Scan Program Files directories for JetBrains IDEs and the default Android Studio install.
    let program_dirs = windows_program_files_dirs();

    for program_dir in &program_dirs {
        let jetbrains_dir = program_dir.join("JetBrains");
        if jetbrains_dir.exists()
            && let Ok(entries) = std::fs::read_dir(&jetbrains_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    for ide in JETBRAINS_IDES {
                        if let Some(detected_ide) = detect_windows_ide(ide, &path)
                            && !detected
                                .iter()
                                .any(|d| d.install_path == detected_ide.install_path)
                        {
                            detected.push(detected_ide);
                        }
                    }
                }
            }
        }
    }

    let android_studio = android_studio_ide();
    for install_path in windows_android_studio_installation_candidates(&program_dirs) {
        if let Some(detected_ide) = detect_windows_ide(android_studio, &install_path)
            && !detected
                .iter()
                .any(|d| d.install_path == detected_ide.install_path)
        {
            detected.push(detected_ide);
        }
    }

    detected
}

#[cfg(windows)]
pub(super) fn scan_windows_toolbox_dir(toolbox_apps: &Path) -> Vec<DetectedIde> {
    let mut detected = Vec::new();

    if let Ok(entries) = std::fs::read_dir(toolbox_apps) {
        for entry in entries.flatten() {
            let app_dir = entry.path();
            if !app_dir.is_dir() {
                continue;
            }

            // Find matching IDE by toolbox app name
            let dir_name = app_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");

            for ide in JETBRAINS_IDES {
                if dir_name.contains(ide.toolbox_app_name)
                    && let Ok(versions) = std::fs::read_dir(&app_dir)
                {
                    for version_entry in versions.flatten() {
                        let version_dir = version_entry.path();
                        if version_dir.is_dir()
                            && let Some(detected_ide) = detect_windows_ide(ide, &version_dir)
                        {
                            detected.push(detected_ide);
                        }
                    }
                }
            }
        }
    }

    detected
}

#[cfg(windows)]
pub(super) fn detect_windows_ide(
    ide: &'static JetBrainsIde,
    install_path: &Path,
) -> Option<DetectedIde> {
    let binary_path = install_path.join("bin").join(ide.binary_name_windows);

    if !binary_path.exists() {
        return None;
    }

    let (build_number, major_build, data_directory_name) =
        read_product_info_build_metadata(&install_path.join("product-info.json"));
    let plugins_dir = get_plugins_dir(
        data_directory_name.as_deref(),
        ide.product_code,
        build_number.as_deref(),
    );

    Some(DetectedIde {
        ide,
        install_path: install_path.to_path_buf(),
        binary_path,
        build_number,
        major_build,
        plugins_dir,
    })
}

#[cfg(windows)]
pub(super) fn windows_program_files_dirs() -> Vec<PathBuf> {
    [
        std::env::var("ProgramFiles").ok(),
        std::env::var("ProgramFiles(x86)").ok(),
    ]
    .into_iter()
    .flatten()
    .map(PathBuf::from)
    .collect()
}

#[cfg(windows)]
pub(super) fn android_studio_ide() -> &'static JetBrainsIde {
    JETBRAINS_IDES
        .iter()
        .find(|ide| ide.product_code == "AI")
        .expect("Android Studio must remain in JETBRAINS_IDES")
}

#[cfg(windows)]
pub(super) fn windows_android_studio_installation_candidates(
    program_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    let mut candidates = default_windows_android_studio_install_paths(program_dirs);
    for candidate in read_windows_android_studio_registry_candidates() {
        push_unique_path(&mut candidates, candidate);
    }
    candidates
}

#[cfg(windows)]
pub(super) fn default_windows_android_studio_install_paths(
    program_dirs: &[PathBuf],
) -> Vec<PathBuf> {
    program_dirs
        .iter()
        .map(|program_dir| program_dir.join("Android").join("Android Studio"))
        .collect()
}

#[cfg(windows)]
pub(super) fn read_windows_android_studio_registry_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for hive in [
        RegKey::predef(HKEY_CURRENT_USER),
        RegKey::predef(HKEY_LOCAL_MACHINE),
    ] {
        for candidate in collect_windows_android_studio_paths_from_hive(&hive) {
            push_unique_path(&mut candidates, candidate);
        }
    }
    candidates
}

#[cfg(windows)]
pub(super) fn collect_windows_android_studio_paths_from_hive(hive: &RegKey) -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(android_studio_key) = hive.open_subkey("Software\\Android Studio")
        && let Some(path) = read_windows_string_value(&android_studio_key, "Path")
        && let Some(candidate) = normalize_windows_install_path_candidate(&path)
    {
        push_unique_path(&mut candidates, candidate);
    }

    for uninstall_path in [
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        "Software\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    ] {
        if let Ok(uninstall_root) = hive.open_subkey(uninstall_path) {
            for subkey_name in uninstall_root.enum_keys().flatten() {
                if let Ok(uninstall_entry) = uninstall_root.open_subkey(&subkey_name)
                    && read_windows_string_value(&uninstall_entry, "DisplayName")
                        .as_deref()
                        .is_some_and(|name| name.contains("Android Studio"))
                {
                    for value_name in ["InstallLocation", "DisplayIcon"] {
                        if let Some(value) = read_windows_string_value(&uninstall_entry, value_name)
                            && let Some(candidate) =
                                normalize_windows_install_path_candidate(&value)
                        {
                            push_unique_path(&mut candidates, candidate);
                        }
                    }
                }
            }
        }
    }

    candidates
}

#[cfg(windows)]
pub(super) fn read_windows_string_value(key: &RegKey, value_name: &str) -> Option<String> {
    key.get_value::<String, _>(value_name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(windows)]
pub(super) fn normalize_windows_install_path_candidate(raw_value: &str) -> Option<PathBuf> {
    let trimmed = raw_value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let path = if let Some(quoted) = trimmed.strip_prefix('"') {
        let end_quote = quoted.find('"')?;
        PathBuf::from(&quoted[..end_quote])
    } else {
        PathBuf::from(trimmed.split(',').next().unwrap_or(trimmed).trim())
    };

    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());

    match file_name.as_deref() {
        Some("studio.exe") | Some("studio64.exe") => path.parent()?.parent().map(Path::to_path_buf),
        Some("bin") => path.parent().map(Path::to_path_buf),
        Some(_) if path.extension().is_some() => None,
        _ => Some(path),
    }
}

#[cfg(windows)]
pub(super) fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}
