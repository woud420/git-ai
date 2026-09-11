use super::is_plugin_installed;
use super::metadata::*;
#[cfg(windows)]
use super::windows::*;
use crate::operations::mdm::jetbrains::ide_types::{DetectedIde, JETBRAINS_IDES};
use std::path::{Path, PathBuf};

#[test]
fn test_read_product_info_build_metadata() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("product-info.json");
    assert_eq!(read_product_info_build_metadata(&path), (None, None, None));
    std::fs::create_dir(&path).unwrap();
    assert_eq!(read_product_info_build_metadata(&path), (None, None, None));
    std::fs::remove_dir(&path).unwrap();
    for (json, expected) in [
        ("{", (None, None, None)),
        (r#"{}"#, (None, None, None)),
        (r#"{"buildNumber":252}"#, (None, None, None)),
        (r#"{"buildNumber":"x"}"#, (Some("x".into()), None, None)),
        (
            r#"{"buildNumber":"252.1"}"#,
            (Some("252.1".into()), Some(252), None),
        ),
        (
            r#"{"buildNumber":"2","dataDirectoryName":"I"}"#,
            (Some("2".into()), Some(2), Some("I".into())),
        ),
    ] {
        std::fs::write(&path, json).unwrap();
        assert_eq!(read_product_info_build_metadata(&path), expected);
    }
}

#[test]
fn test_plugin_version_suffix_prefers_product_info_data_directory_name() {
    let version_suffix =
        plugin_version_suffix(Some("IntelliJIdea2026.1"), "IU", Some("261.22158.277"));
    assert_eq!(version_suffix, "IntelliJIdea2026.1");
}

#[test]
fn test_plugin_version_suffix_falls_back_to_product_code_when_data_directory_name_missing() {
    let version_suffix = plugin_version_suffix(None, "IU", Some("252.27397.103"));
    assert_eq!(version_suffix, "IU2025.2");
}

#[test]
fn test_plugins_dir_for_windows_keeps_jetbrains_parent_for_regular_ides() {
    let plugins_dir = plugins_dir_for_platform(
        JetBrainsPlatform::Windows,
        Path::new("home"),
        Some(Path::new("appdata")),
        Some("IntelliJIdea2026.1"),
        "IU",
        Some("261.22158.277"),
    );
    assert_eq!(
        plugins_dir,
        PathBuf::from("appdata")
            .join("JetBrains")
            .join("IntelliJIdea2026.1")
            .join("plugins")
    );
}

#[test]
fn test_plugins_dir_for_windows_uses_google_parent_for_android_studio() {
    let plugins_dir = plugins_dir_for_platform(
        JetBrainsPlatform::Windows,
        Path::new("home"),
        Some(Path::new("appdata")),
        Some("AndroidStudio2025.3.3"),
        "AI",
        Some("253.31033.145"),
    );
    assert_eq!(
        plugins_dir,
        PathBuf::from("appdata")
            .join("Google")
            .join("AndroidStudio2025.3.3")
            .join("plugins")
    );
}

#[test]
fn test_plugins_dir_for_macos_uses_google_parent_for_android_studio() {
    let plugins_dir = plugins_dir_for_platform(
        JetBrainsPlatform::Macos,
        Path::new("home"),
        None,
        Some("AndroidStudio2025.3.3"),
        "AI",
        Some("253.31033.145"),
    );
    assert_eq!(
        plugins_dir,
        PathBuf::from("home")
            .join("Library")
            .join("Application Support")
            .join("Google")
            .join("AndroidStudio2025.3.3")
            .join("plugins")
    );
}

#[test]
fn test_plugins_dir_for_linux_uses_google_parent_for_android_studio_without_plugins_suffix() {
    let plugins_dir = plugins_dir_for_platform(
        JetBrainsPlatform::Linux,
        Path::new("home"),
        None,
        Some("AndroidStudio2025.3.3"),
        "AI",
        Some("253.31033.145"),
    );
    assert_eq!(
        plugins_dir,
        PathBuf::from("home")
            .join(".local")
            .join("share")
            .join("Google")
            .join("AndroidStudio2025.3.3")
    );
}

#[test]
fn test_plugins_dir_for_linux_keeps_documented_jetbrains_plugins_root() {
    let plugins_dir = plugins_dir_for_platform(
        JetBrainsPlatform::Linux,
        Path::new("home"),
        None,
        Some("WebStorm2026.1"),
        "WS",
        Some("261.24980.77"),
    );
    assert_eq!(
        plugins_dir,
        PathBuf::from("home")
            .join(".local")
            .join("share")
            .join("JetBrains")
            .join("WebStorm2026.1")
    );
}

#[test]
fn test_is_plugin_installed_detects_legacy_extracted_directory() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("git-ai-intellij")).unwrap();

    let detected = DetectedIde {
        ide: &JETBRAINS_IDES[0],
        install_path: PathBuf::from("install"),
        binary_path: PathBuf::from("binary"),
        build_number: Some("261.22158.277".to_string()),
        major_build: Some(261),
        plugins_dir: temp.path().to_path_buf(),
    };

    assert!(is_plugin_installed(&detected));
}

#[test]
fn test_is_plugin_installed_detects_marketplace_directory_name() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("Git AI")).unwrap();

    let detected = DetectedIde {
        ide: &JETBRAINS_IDES[0],
        install_path: PathBuf::from("install"),
        binary_path: PathBuf::from("binary"),
        build_number: Some("261.22158.277".to_string()),
        major_build: Some(261),
        plugins_dir: temp.path().to_path_buf(),
    };

    assert!(is_plugin_installed(&detected));
}

#[cfg(windows)]
#[test]
fn test_default_windows_android_studio_install_paths_cover_both_program_files_roots() {
    let program_dirs = vec![
        PathBuf::from(r"C:\Program Files"),
        PathBuf::from(r"C:\Program Files (x86)"),
    ];
    let candidates = default_windows_android_studio_install_paths(&program_dirs);
    assert_eq!(
        candidates,
        vec![
            PathBuf::from(r"C:\Program Files\Android\Android Studio"),
            PathBuf::from(r"C:\Program Files (x86)\Android\Android Studio"),
        ]
    );
}

#[cfg(windows)]
#[test]
fn test_normalize_windows_install_path_candidate_accepts_install_root() {
    assert_eq!(
        normalize_windows_install_path_candidate(r"D:\software\as"),
        Some(PathBuf::from(r"D:\software\as"))
    );
}

#[cfg(windows)]
#[test]
fn test_normalize_windows_install_path_candidate_strips_bin_and_executable_suffixes() {
    assert_eq!(
        normalize_windows_install_path_candidate(r"D:\software\as\bin"),
        Some(PathBuf::from(r"D:\software\as"))
    );
    assert_eq!(
        normalize_windows_install_path_candidate(r#""D:\software\as\bin\studio64.exe",0"#),
        Some(PathBuf::from(r"D:\software\as"))
    );
    assert_eq!(
        normalize_windows_install_path_candidate(r"D:\software\as\bin\studio.exe"),
        Some(PathBuf::from(r"D:\software\as"))
    );
}
