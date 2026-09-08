mod metadata;
#[cfg(target_os = "macos")]
use metadata::parse_major_build;
#[cfg(unix)]
use metadata::{get_plugins_dir, read_product_info_build_metadata};
#[cfg(windows)]
mod windows;
use super::ide_types::DetectedIde;
#[cfg(unix)]
use super::ide_types::{JETBRAINS_IDES, JetBrainsIde};
#[cfg(unix)]
use crate::operations::mdm::paths::home_dir;
#[cfg(unix)]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(windows)]
use windows::find_windows_installations;

/// Find all installed JetBrains IDEs on the system
pub fn find_jetbrains_installations() -> Vec<DetectedIde> {
    let mut detected = Vec::new();

    #[cfg(target_os = "macos")]
    {
        detected.extend(find_macos_installations());
    }

    #[cfg(windows)]
    {
        detected.extend(find_windows_installations());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        detected.extend(find_linux_installations());
    }

    detected
}

// ===== macOS Detection =====

#[cfg(target_os = "macos")]
fn find_macos_installations() -> Vec<DetectedIde> {
    let mut detected = Vec::new();

    for ide in JETBRAINS_IDES {
        for bundle_id in ide.bundle_ids {
            if let Some(app_path) = find_app_by_bundle_id(bundle_id)
                && let Some(detected_ide) = detect_macos_ide(ide, &app_path)
            {
                detected.push(detected_ide);
            }
        }
    }

    // Also scan common installation directories
    let scan_dirs = vec![
        PathBuf::from("/Applications"),
        home_dir().join("Applications"),
        home_dir().join("Applications/JetBrains Toolbox"),
    ];

    for scan_dir in scan_dirs {
        if scan_dir.exists()
            && let Ok(entries) = std::fs::read_dir(&scan_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "app") {
                    for ide in JETBRAINS_IDES {
                        if is_matching_macos_app(ide, &path)
                            && let Some(detected_ide) = detect_macos_ide(ide, &path)
                        {
                            // Avoid duplicates
                            if !detected
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
    }

    detected
}

#[cfg(target_os = "macos")]
fn find_app_by_bundle_id(bundle_id: &str) -> Option<PathBuf> {
    let output = Command::new("mdfind")
        .args([&format!("kMDItemCFBundleIdentifier == '{}'", bundle_id)])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn is_matching_macos_app(ide: &JetBrainsIde, app_path: &Path) -> bool {
    let app_name = app_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

    let app_name_lower = app_name.to_lowercase();

    // Match based on IDE name patterns
    match ide.product_code {
        "IU" | "IC" => app_name_lower.contains("intellij"),
        "PY" | "PC" => app_name_lower.contains("pycharm"),
        "WS" => app_name_lower.contains("webstorm"),
        "GO" => app_name_lower.contains("goland"),
        "CL" => app_name_lower.contains("clion"),
        "PS" => app_name_lower.contains("phpstorm"),
        "RD" => app_name_lower.contains("rider"),
        "RM" => app_name_lower.contains("rubymine"),
        "DB" => app_name_lower.contains("datagrip"),
        "AI" => app_name_lower.contains("android studio"),
        _ => false,
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_ide(ide: &'static JetBrainsIde, app_path: &Path) -> Option<DetectedIde> {
    let binary_path = app_path
        .join("Contents")
        .join("MacOS")
        .join(ide.binary_name_macos);

    if !binary_path.exists() {
        tracing::debug!(
            "JetBrains: Binary not found at {:?} for {}",
            binary_path,
            ide.name
        );
        return None;
    }

    // Get build number and data directory from Info.plist or product-info.json
    let (build_number, major_build, data_directory_name) = get_macos_build_metadata(app_path);

    // Get plugins directory
    let plugins_dir = get_plugins_dir(
        data_directory_name.as_deref(),
        ide.product_code,
        build_number.as_deref(),
    );

    Some(DetectedIde {
        ide,
        install_path: app_path.to_path_buf(),
        binary_path,
        build_number,
        major_build,
        plugins_dir,
    })
}

#[cfg(target_os = "macos")]
fn get_macos_build_metadata(app_path: &Path) -> (Option<String>, Option<u32>, Option<String>) {
    // Try product-info.json first (newer JetBrains IDEs)
    let product_info_path = app_path.join("Contents/Resources/product-info.json");
    let product_info_metadata = read_product_info_build_metadata(&product_info_path);
    if product_info_metadata.0.is_some() {
        return product_info_metadata;
    }

    // Fall back to Info.plist
    let output = Command::new("defaults")
        .args([
            "read",
            &app_path.join("Contents/Info.plist").to_string_lossy(),
            "CFBundleVersion",
        ])
        .output()
        .ok();

    if let Some(output) = output
        && output.status.success()
    {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let major = parse_major_build(&version);
        return (Some(version), major, None);
    }

    (None, None, None)
}

// ===== Linux Detection =====

#[cfg(all(unix, not(target_os = "macos")))]
fn find_linux_installations() -> Vec<DetectedIde> {
    let mut detected = Vec::new();

    // Scan Toolbox directory
    let toolbox_apps = home_dir()
        .join(".local")
        .join("share")
        .join("JetBrains")
        .join("Toolbox")
        .join("apps");

    if toolbox_apps.exists() {
        detected.extend(scan_linux_toolbox_dir(&toolbox_apps));
    }

    // Scan common installation directories
    let scan_dirs = vec![
        home_dir().join(".local").join("share").join("JetBrains"),
        PathBuf::from("/opt"),
        PathBuf::from("/usr/local"),
    ];

    for scan_dir in scan_dirs {
        if scan_dir.exists()
            && let Ok(entries) = std::fs::read_dir(&scan_dir)
        {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    for ide in JETBRAINS_IDES {
                        if let Some(detected_ide) = detect_linux_ide(ide, &path)
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

    detected
}

#[cfg(all(unix, not(target_os = "macos")))]
fn scan_linux_toolbox_dir(toolbox_apps: &Path) -> Vec<DetectedIde> {
    let mut detected = Vec::new();

    if let Ok(entries) = std::fs::read_dir(toolbox_apps) {
        for entry in entries.flatten() {
            let app_dir = entry.path();
            if !app_dir.is_dir() {
                continue;
            }

            let dir_name = app_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");

            for ide in JETBRAINS_IDES {
                if dir_name.contains(ide.toolbox_app_name) {
                    // Toolbox uses versioned subdirectories with a "ch-0" pattern
                    if let Ok(channels) = std::fs::read_dir(&app_dir) {
                        for channel_entry in channels.flatten() {
                            let channel_dir = channel_entry.path();
                            if channel_dir.is_dir() {
                                // Inside channel, there are version directories
                                if let Ok(versions) = std::fs::read_dir(&channel_dir) {
                                    for version_entry in versions.flatten() {
                                        let version_dir = version_entry.path();
                                        if version_dir.is_dir()
                                            && let Some(detected_ide) =
                                                detect_linux_ide(ide, &version_dir)
                                        {
                                            detected.push(detected_ide);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    detected
}

#[cfg(all(unix, not(target_os = "macos")))]
fn detect_linux_ide(ide: &'static JetBrainsIde, install_path: &Path) -> Option<DetectedIde> {
    let binary_path = install_path.join("bin").join(ide.binary_name_linux);

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

/// Check if the Git AI plugin is installed for a detected IDE
pub fn is_plugin_installed(detected: &DetectedIde) -> bool {
    // Support both the legacy extracted directory name and the Marketplace-installed
    // directory name that JetBrains/Android Studio writes on disk.
    ["git-ai-intellij", "Git AI"]
        .into_iter()
        .map(|dir_name| detected.plugins_dir.join(dir_name))
        .any(|plugin_dir| plugin_dir.exists())
}

#[cfg(test)]
mod tests;
