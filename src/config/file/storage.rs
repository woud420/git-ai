use super::FileConfig;
use crate::operations::mdm::paths::home_dir;
use std::fs;
use std::path::PathBuf;

pub(crate) fn load_file_config() -> Option<FileConfig> {
    let data = fs::read(config_file_path()?).ok()?;
    parse_file_config_bytes(&data).ok()
}

/// Master telemetry switch resolution: off unless explicitly enabled via the
/// `telemetry` key ("on"/"off") or the legacy `telemetry_oss` key ("on").
pub(crate) fn resolve_telemetry_enabled(telemetry: Option<&str>, legacy_oss: Option<&str>) -> bool {
    match telemetry.map(str::trim) {
        Some("on") => true,
        Some("off") => false,
        Some(other) => {
            eprintln!("Warning: Invalid telemetry value '{}', using 'off'", other);
            false
        }
        None => legacy_oss.map(str::trim) == Some("on"),
    }
}

/// Strip a leading UTF-8 byte-order mark (`EF BB BF`, the encoding of
/// `'\u{feff}'`) from `data`, if present. Shared by config-file parsing
/// (Windows PowerShell 5.1 writes UTF-8 with BOM by default for
/// `Out-File -Encoding UTF8`) and checkpoint hook-input decoding, both of
/// which need to tolerate BOM-prefixed input.
pub(crate) fn strip_utf8_bom(data: &[u8]) -> &[u8] {
    data.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(data)
}

pub(crate) fn parse_file_config_bytes(data: &[u8]) -> Result<FileConfig, serde_json::Error> {
    serde_json::from_slice::<FileConfig>(strip_utf8_bom(data))
}

pub fn config_file_path() -> Option<PathBuf> {
    Some(home_dir().join(".git-ai").join("config.json"))
}

/// Load the raw file config
pub fn load_file_config_public() -> Result<FileConfig, String> {
    let path =
        config_file_path().ok_or_else(|| "Could not determine config file path".to_string())?;

    if !path.exists() {
        // Return empty config if file doesn't exist
        return Ok(FileConfig::default());
    }

    let data = fs::read(&path).map_err(|e| format!("Failed to read config file: {}", e))?;

    parse_file_config_bytes(&data).map_err(|e| format!("Failed to parse config file: {}", e))
}

/// Save the file config
pub fn save_file_config(config: &FileConfig) -> Result<(), String> {
    let path =
        config_file_path().ok_or_else(|| "Could not determine config file path".to_string())?;

    // Ensure the directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&path, json).map_err(|e| format!("Failed to write config file: {}", e))
}
