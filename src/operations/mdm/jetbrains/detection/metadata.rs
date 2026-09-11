use crate::operations::mdm::paths::home_dir;
use std::path::{Path, PathBuf};

// ===== Shared Utilities =====

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum JetBrainsPlatform {
    Macos,
    Windows,
    Linux,
}

pub(super) fn read_product_info_build_metadata(
    product_info_path: &Path,
) -> (Option<String>, Option<u32>, Option<String>) {
    if !product_info_path.exists() {
        return Default::default();
    }
    std::fs::read_to_string(product_info_path)
        .ok()
        .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
        .and_then(|json| {
            let build = json["buildNumber"].as_str()?.to_owned();
            let major_build = parse_major_build(&build);
            let data_directory_name = json["dataDirectoryName"].as_str().map(str::to_owned);
            Some((Some(build), major_build, data_directory_name))
        })
        .unwrap_or_default()
}

/// Parse the major build number from a build string like "252.12345.67"
pub(super) fn parse_major_build(build: &str) -> Option<u32> {
    build.split('.').next()?.parse().ok()
}

pub(super) fn plugin_version_suffix(
    data_directory_name: Option<&str>,
    product_code: &str,
    build_number: Option<&str>,
) -> String {
    // Prefer the IDE's real dataDirectoryName from product-info.json when available.
    // This matches the actual config/plugins directory used by modern JetBrains IDEs
    // (for example "IntelliJIdea2026.1"), avoiding incorrect guesses like "IU2026.1".
    data_directory_name
        .map(ToOwned::to_owned)
        .or_else(|| {
            build_number.and_then(parse_major_build).map(|major| {
                // Build 252 = 2025.2, 251 = 2025.1, 243 = 2024.3, etc.
                let year = 2000 + (major / 10);
                let minor = major % 10;
                format!("{}{}.{}", product_code, year, minor)
            })
        })
        .unwrap_or_else(|| product_code.to_string())
}

pub(super) fn plugins_parent_dir_name(product_code: &str) -> &'static str {
    match product_code {
        // Android Studio stores its user directories under Google instead of JetBrains.
        "AI" => "Google",
        _ => "JetBrains",
    }
}

pub(super) fn plugins_dir_for_platform(
    platform: JetBrainsPlatform,
    home_dir: &Path,
    appdata: Option<&Path>,
    data_directory_name: Option<&str>,
    product_code: &str,
    build_number: Option<&str>,
) -> PathBuf {
    let version_suffix = plugin_version_suffix(data_directory_name, product_code, build_number);
    let parent_dir = plugins_parent_dir_name(product_code);

    match platform {
        JetBrainsPlatform::Macos => home_dir
            .join("Library")
            .join("Application Support")
            .join(parent_dir)
            .join(&version_suffix)
            .join("plugins"),
        JetBrainsPlatform::Windows => appdata
            .map(Path::to_path_buf)
            .unwrap_or_else(|| home_dir.join("AppData").join("Roaming"))
            .join(parent_dir)
            .join(&version_suffix)
            .join("plugins"),
        // Linux stores user-installed plugins directly in the share directory root.
        JetBrainsPlatform::Linux => home_dir
            .join(".local")
            .join("share")
            .join(parent_dir)
            .join(&version_suffix),
    }
}

/// Get the plugins directory for an IDE
pub(super) fn get_plugins_dir(
    data_directory_name: Option<&str>,
    product_code: &str,
    build_number: Option<&str>,
) -> PathBuf {
    let home = home_dir();

    #[cfg(target_os = "macos")]
    {
        plugins_dir_for_platform(
            JetBrainsPlatform::Macos,
            &home,
            None,
            data_directory_name,
            product_code,
            build_number,
        )
    }

    #[cfg(windows)]
    {
        let appdata = std::env::var("APPDATA").ok();
        plugins_dir_for_platform(
            JetBrainsPlatform::Windows,
            &home,
            appdata.as_deref().map(Path::new),
            data_directory_name,
            product_code,
            build_number,
        )
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        plugins_dir_for_platform(
            JetBrainsPlatform::Linux,
            &home,
            None,
            data_directory_name,
            product_code,
            build_number,
        )
    }
}
